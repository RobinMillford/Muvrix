#!/usr/bin/env python3
"""Generate a CycloneDX SBOM and license inventory for Muvrix Desktop.

The SBOM is derived from Cargo.lock (exact resolved versions) and the license
fields of the corresponding crate manifests in the local cargo registry. It
covers the Rust dependency graph; the private media runtime is recorded
separately from target/runtime/runtime-manifest.json when it exists, so the
shipped binary dependencies appear in the same document.

Usage:
    python3 packaging/generate_sbom.py [--output DIR]

Outputs:
    <DIR>/sbom.cdx.json        CycloneDX 1.5 JSON
    <DIR>/licenses.md          dependency -> license inventory
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import sys
import tomllib
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
SERIAL_PREFIX = "urn:uuid:muvrix-sbom"


def parse_lock(path: Path) -> list[dict]:
    data = tomllib.loads(path.read_text(encoding="utf-8"))
    packages = []
    for pkg in data.get("package", []):
        name = pkg.get("name", "")
        source = pkg.get("source", "")
        if not name:
            continue
        # Local workspace members have no source and no checksum.
        if not source:
            continue
        packages.append({
            "name": name,
            "version": pkg.get("version", ""),
            "source": source,
            "checksum": pkg.get("checksum"),
        })
    return packages


def registry_index() -> dict[str, Path]:
    """Map (name, version) to the unpacked crate directory."""
    index: dict[str, Path] = {}
    cargo_home = Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo"))
    registry = cargo_home / "registry" / "src"
    if not registry.is_dir():
        return index
    for host_dir in registry.iterdir():
        if not host_dir.is_dir():
            continue
        for crate_dir in host_dir.iterdir():
            if not crate_dir.is_dir():
                continue
            manifest = crate_dir / "Cargo.toml"
            if not manifest.is_file():
                continue
            index[f"{crate_dir.name}"] = crate_dir
    return index


def crate_metadata(crate_dir: Path) -> tuple[str | None, str | None]:
    manifest = crate_dir / "Cargo.toml"
    try:
        data = tomllib.loads(manifest.read_text(encoding="utf-8", errors="replace"))
    except Exception:
        return None, None
    package = data.get("package", {})
    license_field = package.get("license")
    if isinstance(license_field, list):
        license_field = " OR ".join(license_field)
    repository = package.get("repository") or package.get("homepage")
    return license_field, repository


def purl(name: str, version: str, source: str) -> str:
    if "crates.io" in source:
        return f"pkg:cargo/{name}@{version}"
    return f"pkg:cargo/{name}@{version}?source={source}"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", default=str(REPO_ROOT / "docs/release"))
    args = parser.parse_args()

    out_dir = Path(args.output)
    out_dir.mkdir(parents=True, exist_ok=True)

    packages = parse_lock(REPO_ROOT / "Cargo.lock")
    index = registry_index()

    components = []
    license_rows = []
    missing_license = []

    for pkg in packages:
        key = f"{pkg['name']}-{pkg['version']}"
        crate_dir = index.get(key)
        license_id, repository = (None, None)
        if crate_dir is not None:
            license_id, repository = crate_metadata(crate_dir)

        if not license_id:
            missing_license.append(f"{pkg['name']} {pkg['version']}")

        component = {
            "type": "library",
            "bom-ref": purl(pkg["name"], pkg["version"], pkg["source"]),
            "name": pkg["name"],
            "version": pkg["version"],
            "purl": purl(pkg["name"], pkg["version"], pkg["source"]),
            "scope": "required",
        }
        if repository:
            component["externalReferences"] = [
                {"type": "vcs", "url": repository},
            ]
        if license_id:
            component["licenses"] = [{"license": {"id": license_id}}]
        properties = [{"name": "muvrix:source", "value": pkg["source"]}]
        if pkg.get("checksum"):
            properties.append({"name": "muvrix:checksum", "value": pkg["checksum"]})
        component["properties"] = properties
        components.append(component)
        license_rows.append((pkg["name"], pkg["version"], license_id or "UNKNOWN", repository or ""))

    # Private runtime (libmpv/libplacebo/ffmpeg ...) is not in Cargo.lock.
    runtime_manifest = REPO_ROOT / "target" / "runtime" / "runtime-manifest.json"
    runtime_components = []
    data = {}
    if runtime_manifest.is_file():
        data = json.loads(runtime_manifest.read_text(encoding="utf-8"))
    if "libmpv" in data and "libplacebo" in data:
        runtime_components.append({
            "type": "library",
            "bom-ref": f"pkg:generic/libmpv@{data['libmpv']['version']}",
            "name": "libmpv",
            "version": data["libmpv"]["version"],
            "purl": f"pkg:generic/libmpv@{data['libmpv']['version']}",
            "scope": "required",
            "properties": [
                {"name": "muvrix:client-api", "value": data["libmpv"]["client_api"]},
                {"name": "muvrix:origin", "value": data["libmpv"]["built_from"]},
            ],
        })
        runtime_components.append({
            "type": "library",
            "bom-ref": f"pkg:generic/libplacebo@{data['libplacebo']['version']}",
            "name": "libplacebo",
            "version": data["libplacebo"]["version"],
            "purl": f"pkg:generic/libplacebo@{data['libplacebo']['version']}",
            "scope": "required",
            "properties": [{"name": "muvrix:origin", "value": data["libplacebo"]["built_from"]}],
        })
        seen = {"libmpv", "libplacebo"}
        for entry in data.get("files", []):
            pkg_name = entry.get("package") or ""
            if not pkg_name or pkg_name in seen:
                continue
            seen.add(pkg_name)
            runtime_components.append({
                "type": "library",
                "bom-ref": f"pkg:deb/ubuntu/{pkg_name}@{entry.get('version') or 'unknown'}",
                "name": pkg_name,
                "version": entry.get("version") or "unknown",
                "purl": f"pkg:deb/ubuntu/{pkg_name}@{entry.get('version') or 'unknown'}",
                "scope": "required",
                "hashes": [{"alg": "SHA-256", "content": entry["sha256"]}],
            })

    sbom = {
        "bomFormat": "CycloneDX",
        "specVersion": "1.5",
        "serialNumber": f"{SERIAL_PREFIX}-{hashlib.sha256(str(len(components)).encode()).hexdigest()[:24]}",
        "version": 1,
        "metadata": {
            "component": {
                "type": "application",
                "bom-ref": "pkg:generic/muvrix@0.1.0",
                "name": "muvrix",
                "version": "0.1.0",
                "description": "Native desktop media player built in Rust on libmpv 0.42 (client API 2.6)",
                "licenses": [{"license": {"id": "GPL-3.0-or-later"}}],
            },
            "tools": [{"name": "packaging/generate_sbom.py", "version": "1"}],
            "properties": [
                {"name": "muvrix:rust-dependencies", "value": str(len(components))},
                {"name": "muvrix:runtime-libraries", "value": str(len(runtime_components))},
            ],
        },
        "components": components + runtime_components,
    }

    sbom_path = out_dir / "sbom.cdx.json"
    sbom_path.write_text(json.dumps(sbom, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    lines = [
        "# Muvrix Desktop dependency licenses",
        "",
        f"Generated from `Cargo.lock` ({len(components)} crates) and the private runtime payload"
        f" ({len(runtime_components)} libraries).",
        "",
        "| Component | Version | License | Repository |",
        "|-----------|---------|---------|------------|",
    ]
    for name, version, lic, repo in sorted(license_rows):
        lines.append(f"| {name} | {version} | {lic} | {repo} |")
    lines.append("")
    if missing_license:
        lines.append("## Crates without a declared license field")
        lines.append("")
        for item in missing_license:
            lines.append(f"- {item}")
        lines.append("")
    lines.append("## Private media runtime")
    lines.append("")
    lines.append("| Component | Version | License | Origin |")
    lines.append("|-----------|---------|---------|--------|")
    lines.append("| libmpv (mpv) | 0.42.0 | GPL-2.0-or-later / LGPL-2.1-or-later | built from pinned source |")
    lines.append("| libplacebo | 7.387.0 | LGPL-2.1-or-later | built from pinned source |")
    lines.append("| FFmpeg libraries | 6.1.1 (Ubuntu 24.04) | LGPL-2.1-or-later / GPL-2.0-or-later | bundled privately, license text shipped |")
    lines.append("")
    (out_dir / "licenses.md").write_text("\n".join(lines), encoding="utf-8")

    print(f"[+] SBOM: {sbom_path} ({len(components)} crates, {len(runtime_components)} runtime libraries)")
    print(f"[+] Licenses: {out_dir / 'licenses.md'}")
    if missing_license:
        print(f"[!] {len(missing_license)} crates have no declared license field", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
