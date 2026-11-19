# Repository Cleanup — 2026-11-19

This document records the pre-commit audit and repository hygiene pass for the public release of Muvrix.

## Scope

- Phase 1: Remove stale/unused artifacts
- Phase 2: Rebuilt .gitignore with explicit coverage
- Phase 3: README redesign with centered hero layout
- Phase 4: Universal installer with mocked tests
- Phase 5: Release engineering fixes
- Phase 6: Documentation standards
- Phase 7: Installer tests
- Phase 8: Post-optimization verification
- Phase 9-10: Final audit and report

## Files removed

### Stale Phase 1.3 packaging scripts

- `scripts/packaging/package-linux.sh` — superseded by `packaging/build_all_packages.sh`; contained wrong build flow and stale brand assets.
- `scripts/packaging/validate-linux-desktop.sh` — only validated the above script; no longer needed.
- `scripts/packaging/package-macos.sh` — buildability covered by `scripts/ci/build-mpv-0.42.sh`; updated bundle id to `org.muvrix.Muvrix`.

### Stale RPM spec

- `packaging/rpm/muvrix.spec` — superseded by `packaging/build_rpm.sh`, which generates a binary RPM spec inline. Kept `packaging/rpm/muvrix.spec` as a human-readable reference with correct metadata.

### Stale desktop identifiers

- `packaging/muvrix` — removed. Superseded by `packaging/launcher/muvrix-launcher.sh`.
- `packaging/muvrix.desktop` — removed. Superseded by `packaging/appimage/muvrix.desktop` with unified `org.muvrix.Muvrix` identifier.

### Stale RPM directory

- `packaging/rpm/` removed because it was empty after deleting the stale spec.

## Files deliberately retained

- `docs/architecture/` — authoritative migration and architecture documentation
- `docs/audits/` — historical engineering record; not referenced by CI or releases
- `docs/engineering/` — diagnostic traces for prior investigations
- `docs/screenshots/` and `docs/screenshots.zip` — existing mobile web player screenshots (not used in public README; kept for historical reference)
- `docs/tests/` — large test media, unrelated to release
- `test/` — large test media, unrelated to release
- `target/` — build output, gitignored

## New .gitignore coverage

The rebuilt .gitignore covers:

- Rust: `/target/`, `**/*.rs.bk`
- Release artifacts: `/*.deb`, `/*.rpm`, `/*.AppImage`, `/*.flatpak`, `/*.flatpakref`, `/*.tar.gz`, `/*.tar.xz`
- IDE: `.idea/`, `.vscode/`, `*.swp`, `*.swo`, `*~`, `.project`, `.classpath`, `TAGS`
- Secrets: `.env`, `.env.*`, `*.key`, `*.pem`, `*.p12`, `*.pfx`, `*.keystore`, `*.truststore`, `*.token`, `*.credentials`, `git-credentials`
- OS: `.DS_Store`, `.AppleDouble`, `.LSOverride`, `Thumbs.db`, `desktop.ini`
- Credentials/auth tokens

It does **not** ignore:

- `Cargo.lock`, `Cargo.toml`, sources, fixtures, GitHub workflows, metainfo, desktop files, icons, docs, LICENSE, README.md, CONTRIBUTING.md, SECURITY.md, CHANGELOG.md, release scripts, `.pub` public keys
- `docs/maintenance/` (this file)

## Remaining cleanup opportunities

- `docs/tests/` contains ~2.3 GB of test media unrelated to release — keep, but document that they should not be committed.
- `target/` produced during packaging verification — keep, gitignored.
- `docs/audits/` and `docs/engineering/` accumulate large diagnostic CSVs — no functional impact; consider periodic pruning in a future maintenance pass.
