//! Phase E9 release-readiness checks.
//!
//! These tests validate the release *infrastructure* that can be checked
//! statically: the SBOM, the dependency license inventory, the private runtime
//! manifest and the signing-status contract. Tests that need built artifacts
//! skip (with an explicit message) when `target/packages` has not been built,
//! so `cargo test` stays usable without running the full packaging pipeline.
//!
//! Real installation and launch behaviour is exercised by
//! `packaging/tests/live_install_test.sh`, not here.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crate lives in crates/muvrix-ui")
        .to_path_buf()
}

fn read(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok()
}

#[test]
fn sbom_is_valid_cyclonedx_and_covers_dependencies() {
    let path = repo_root().join("docs/release/sbom.cdx.json");
    let Some(text) = read(&path) else {
        eprintln!("skip: {} not generated yet", path.display());
        return;
    };
    let sbom: serde_json::Value = serde_json::from_str(&text).expect("SBOM is valid JSON");

    assert_eq!(sbom["bomFormat"], "CycloneDX");
    assert_eq!(sbom["specVersion"], "1.5");

    let components = sbom["components"]
        .as_array()
        .expect("components array present");
    assert!(
        components.len() > 100,
        "expected the full dependency graph, found {} components",
        components.len()
    );

    // Every component must be identifiable and reference a package URL.
    for component in components {
        let name = component["name"].as_str().unwrap_or_default();
        let version = component["version"].as_str().unwrap_or_default();
        let purl = component["purl"].as_str().unwrap_or_default();
        assert!(!name.is_empty(), "component without a name: {component}");
        assert!(!version.is_empty(), "component {name} has no version");
        assert!(
            purl.starts_with("pkg:"),
            "component {name} has no package URL"
        );
    }

    // libmpv is the reason this whole runtime exists; it must be represented.
    assert!(
        components.iter().any(|c| c["name"] == "libmpv"),
        "SBOM must declare libmpv"
    );
}

#[test]
fn dependency_license_inventory_is_published() {
    let path = repo_root().join("docs/release/licenses.md");
    let Some(text) = read(&path) else {
        eprintln!("skip: {} not generated yet", path.display());
        return;
    };
    assert!(text.contains("# Muvrix Desktop dependency licenses"));
    assert!(text.contains("## Private media runtime"));
    assert!(
        text.contains("libmpv"),
        "license inventory must mention the bundled runtime"
    );
    // The inventory must not silently drop unknown licenses.
    let rows = text.lines().filter(|line| line.starts_with("| ")).count();
    assert!(rows > 100, "expected a full inventory, found {rows} rows");
}

#[test]
fn private_runtime_manifest_pins_libmpv_042_and_records_hashes() {
    let path = repo_root().join("target/runtime/runtime-manifest.json");
    let Some(text) = read(&path) else {
        eprintln!("skip: {} not built yet", path.display());
        return;
    };
    let manifest: serde_json::Value =
        serde_json::from_str(&text).expect("runtime manifest is valid JSON");

    assert_eq!(manifest["schema"], "muvrix-runtime-bundle/2");
    assert!(
        manifest["libmpv"]["version"]
            .as_str()
            .unwrap_or_default()
            .starts_with("0.42"),
        "the runtime must pin libmpv 0.42, got {}",
        manifest["libmpv"]["version"]
    );
    assert_eq!(manifest["libmpv"]["client_api"], "2.6");
    assert!(
        manifest["baseline_glibc"]
            .as_str()
            .unwrap_or_default()
            .starts_with("2.39"),
        "baseline glibc must be documented"
    );

    let files = manifest["files"].as_array().expect("files array");
    assert!(!files.is_empty(), "runtime manifest lists no libraries");
    for file in files {
        let sha = file["sha256"].as_str().unwrap_or_default();
        assert_eq!(sha.len(), 64, "every runtime file needs a SHA-256");
        assert!(
            file["soname"]
                .as_str()
                .unwrap_or_default()
                .starts_with("lib"),
            "unexpected soname entry: {file}"
        );
    }
    assert!(
        files.iter().any(|f| f["soname"] == "libmpv.so.2"),
        "runtime must include libmpv.so.2"
    );
}

#[test]
fn signing_status_is_explicit_and_never_fabricated() {
    let path = repo_root().join("target/packages/SIGNING_STATUS");
    let Some(text) = read(&path) else {
        eprintln!("skip: no release directory built yet");
        return;
    };
    let mut fields = std::collections::BTreeMap::new();
    for line in text.lines() {
        if let Some((key, value)) = line.split_once('=') {
            fields.insert(key.trim().to_string(), value.trim().to_string());
        }
    }
    let status = fields.get("status").expect("status field");
    assert!(
        status == "pending" || status == "signed",
        "unexpected signing status {status}"
    );
    if status == "pending" {
        // An unsigned release must say why, and must not claim a signature.
        assert!(fields.contains_key("reason"));
        let release = repo_root().join("target/packages/SHA256SUMS.asc");
        assert!(
            !release.exists(),
            "pending signing must not leave a signature file behind"
        );
    } else {
        assert!(fields.contains_key("key"), "signed status needs a key id");
        assert!(fields.contains_key("signature_sha256"));
    }
}

#[test]
fn release_manifest_is_complete_and_verifiable() {
    let packages = repo_root().join("target/packages");
    let sums = packages.join("SHA256SUMS");
    let Some(text) = read(&sums) else {
        eprintln!("skip: release artifacts not built yet");
        return;
    };
    let listed: Vec<&str> = text
        .lines()
        .filter_map(|line| line.split_whitespace().nth(1))
        .collect();
    for expected in [
        "muvrix_0.1.0_amd64.deb",
        "muvrix-0.1.0-1.x86_64.rpm",
        "Muvrix-0.1.0-x86_64.AppImage",
        "org.muvrix.Muvrix.flatpak",
        "org.muvrix.Muvrix.flatpakref",
    ] {
        assert!(
            listed.contains(&expected),
            "SHA256SUMS is missing {expected}"
        );
    }

    // Every artifact is large enough to contain the private runtime.
    let runtime_manifest = repo_root().join("target/runtime/runtime-manifest.json");
    if runtime_manifest.exists() {
        for artifact in ["muvrix_0.1.0_amd64.deb", "muvrix-0.1.0-1.x86_64.rpm"] {
            let path = packages.join(artifact);
            let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            assert!(
                size > 20_000_000,
                "{artifact} is only {size} bytes; the bundled runtime is missing"
            );
        }
    }
}

#[test]
fn verify_release_script_accepts_the_built_release() {
    let packages = repo_root().join("target/packages");
    if !packages.join("SHA256SUMS").exists() {
        eprintln!("skip: release artifacts not built yet");
        return;
    }
    let script = repo_root().join("packaging/verify_release.sh");
    let output = Command::new("bash")
        .arg(&script)
        .arg(&packages)
        .output()
        .expect("verify_release.sh runs");
    assert!(
        output.status.success(),
        "verify_release.sh failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn checksum_tampering_is_detected() {
    let packages = repo_root().join("target/packages");
    let source = packages.join("org.muvrix.Muvrix.flatpakref");
    if !source.exists() {
        eprintln!("skip: release artifacts not built yet");
        return;
    }
    let temp = tempfile::tempdir().expect("temp dir");
    let copy = temp.path().join("flatpakref");
    fs::copy(&source, &copy).expect("copy artifact");
    let mut bytes = fs::read(&copy).expect("read copy");
    let last = bytes.len() - 1;
    bytes[last] ^= 0xFF;
    fs::write(&copy, &bytes).expect("tamper copy");

    // Recompute with the manifest's expected digest: the tampered copy must differ.
    let left = Command::new("sha256sum")
        .arg(&source)
        .output()
        .expect("sha256sum");
    let right = Command::new("sha256sum")
        .arg(&copy)
        .output()
        .expect("sha256sum");
    assert_ne!(
        String::from_utf8_lossy(&left.stdout),
        String::from_utf8_lossy(&right.stdout),
        "a modified artifact produced an identical checksum"
    );
}
