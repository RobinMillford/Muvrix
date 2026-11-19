# Releasing Muvrix

This document describes how to cut a release.

## Versioning

Muvrix follows SemVer. The version lives in three places and must stay consistent:

- `Cargo.toml` — `version = "0.1.0"` in the `[workspace.package]` section
- `crates/muvrix-ui/Cargo.toml` and other member crates — inherited from workspace
- `packaging/metainfo/org.muvrix.Muvrix.metainfo.xml` — `<release version=...>`

## What CI builds

The `packaging.yml` workflow builds four artifacts on every push to `phase-*`,
`main`, or `master`:

- `org.muvrix.Muvrix.flatpak` — Flatpak bundle
- `muvrix_0.1.0_amd64.deb` — Debian binary package
- `muvrix-0.1.0-1.x86_64.rpm` — Fedora binary RPM
- `Muvrix-0.1.0-x86_64.AppImage` — AppImage

It also produces:

- `SHA256SUMS` — manifest for all artifacts
- `docs/release/sbom.cdx.json` — CycloneDX SBOM
- `docs/release/licenses.md` — vendored dependency license inventory
- Signing status file

All artifacts are verified with their native tooling before upload.

## Cutting a release

1. Update the version in `Cargo.toml`, `packaging/metainfo/org.muvrix.Muvrix.metainfo.xml`, and any changelogs.
2. Run `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings`.
3. Push to `main` or a `phase-*` branch with the bump commit.
4. Wait for CI to finish.
5. Create a GitHub Release tagged `v<version>` (e.g. `v0.1.0`).
6. Attach the four artifacts plus `SHA256SUMS` from the CI run.
7. Publish the release.

## Local release build (manual)

From a clean tree:

```sh
bash packaging/build_runtime_bundle.sh   # if target/runtime is missing
bash packaging/build_all_packages.sh
```

This validates, builds, verifies, and emits all four artifacts into
`target/packages/`.

## Signing

Release signing is handled by `packaging/sign_release.sh`, which is a no-op
unless `MUVRIX_SIGNING_KEY_FILE` and `MUVRIX_SIGNING_PASSPHRASE` are set.
Set them in your local environment to sign the release manifest.
