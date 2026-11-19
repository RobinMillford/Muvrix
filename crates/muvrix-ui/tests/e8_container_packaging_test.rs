//! Phase E8: Multi-Platform Release & Packaging Integration Tests
//!
//! Validates the release artifacts with the native tooling of each format:
//! Debian control parsing, RPM metadata parsing, OSTree Flatpak bundle import,
//! AppImage execution/extraction, and SHA-256 manifest integrity.
//!
//! Real installation into disposable containers is exercised when
//! `MUVRIX_CONTAINER_INSTALL_TESTS=1` is set, because it requires a container
//! engine plus network access to fetch distribution dependencies.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use muvrix_ui::packaging::{PackageFormat, PackagingInfo};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("Failed to locate repository root")
        .to_path_buf()
}

fn have(tool: &str) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {tool} >/dev/null 2>&1"))
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn have_container_engine() -> bool {
    have("podman") || have("docker")
}

fn container_install_tests_enabled() -> bool {
    std::env::var("MUVRIX_CONTAINER_INSTALL_TESTS").is_ok_and(|v| v == "1")
}

fn packages_dir() -> PathBuf {
    repo_root().join("target/packages")
}

/// Renders a `.deb` control file with native dpkg-deb, either on the host or in
/// a disposable Ubuntu container.
fn deb_control(deb: &Path) -> Option<String> {
    let root = repo_root();
    let relative = format!("target/packages/{}", deb.file_name()?.to_string_lossy());
    let output = if have("dpkg-deb") {
        // `--ctrl-tarfile` emits the control member as a tar stream, not the
        // control file itself; the member has to be extracted on both paths or
        // the host branch would compare raw tar bytes against a control stanza.
        Command::new("sh")
            .arg("-c")
            .arg(format!(
                "dpkg-deb --ctrl-tarfile '{}' | tar -xO ./control",
                deb.display()
            ))
            .output()
    } else if have_container_engine() {
        Command::new("podman")
            .args(["run", "--rm", "-v"])
            .arg(format!("{}:/w:z", root.display()))
            .args(["-w", "/w", "ubuntu:24.04", "bash", "-c"])
            .arg(format!(
                "dpkg-deb --ctrl-tarfile '{relative}' | tar -xO ./control"
            ))
            .output()
    } else {
        return None;
    };

    let output = output.ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

#[test]
fn test_deb_control_is_a_binary_stanza() {
    let deb = packages_dir().join("muvrix_0.1.0_amd64.deb");
    if !deb.exists() {
        eprintln!("skipping: {} not built", deb.display());
        return;
    }

    let Some(control) = deb_control(&deb) else {
        eprintln!("skipping: no dpkg-deb or container engine available");
        return;
    };

    // A binary package control file must begin with the binary stanza.
    assert!(
        control.starts_with("Package: muvrix\n"),
        "DEBIAN/control must start with the binary Package field, got:\n{control}"
    );
    assert!(
        !control.lines().any(|line| line.starts_with("Source:")),
        "DEBIAN/control must not contain the source package stanza"
    );
    for field in [
        "Version: 0.1.0",
        "Architecture: amd64",
        "Maintainer:",
        "Depends:",
        "Description:",
    ] {
        assert!(
            control.contains(field),
            "control file missing required field: {field}"
        );
    }
}

#[test]
fn test_deb_real_installation_in_ubuntu() {
    if !container_install_tests_enabled() {
        eprintln!(
            "skipping: set MUVRIX_CONTAINER_INSTALL_TESTS=1 to run container installation tests"
        );
        return;
    }
    let root = repo_root();
    let deb = packages_dir().join("muvrix_0.1.0_amd64.deb");
    if !deb.exists() {
        eprintln!("skipping: {} not built", deb.display());
        return;
    }
    assert!(
        have_container_engine(),
        "container engine required for installation tests"
    );

    let script = r#"
set -e
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
apt-get install -y -qq ./packages/muvrix_0.1.0_amd64.deb
test -x /usr/bin/muvrix
dpkg -s muvrix | grep -qx 'Version: 0.1.0'
dpkg -r muvrix >/dev/null
test ! -e /usr/bin/muvrix
echo INSTALL_UNINSTALL_OK
"#;

    let output = Command::new("podman")
        .args(["run", "--rm", "-v"])
        .arg(format!("{}/target/packages:/packages:z", root.display()))
        .args(["ubuntu:24.04", "bash", "-c", script])
        .output()
        .expect("failed to run container installation test");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success() && stdout.contains("INSTALL_UNINSTALL_OK"),
        "Debian install/uninstall failed.\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

#[test]
fn test_rpm_metadata_parses_with_native_rpm() {
    let rpm = packages_dir().join("muvrix-0.1.0-1.x86_64.rpm");
    if !rpm.exists() {
        eprintln!("skipping: {} not built", rpm.display());
        return;
    }

    let output = if have("rpm") {
        Command::new("rpm").arg("-qip").arg(&rpm).output()
    } else if have_container_engine() {
        Command::new("podman")
            .args(["run", "--rm", "-v"])
            .arg(format!(
                "{}/target/packages:/packages:z",
                repo_root().display()
            ))
            .args([
                "fedora:40",
                "rpm",
                "-qip",
                "/packages/muvrix-0.1.0-1.x86_64.rpm",
            ])
            .output()
    } else {
        eprintln!("skipping: no rpm or container engine available");
        return;
    };

    let output = output.expect("failed to query RPM");
    assert!(output.status.success(), "native rpm rejected the package");
    let info = String::from_utf8_lossy(&output.stdout);
    // rpm pads header keys with varying alignment, so compare normalized pairs.
    let fields: Vec<(String, String)> = info
        .lines()
        .filter_map(|line| line.split_once(':'))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect();
    let value_of = |key: &str| -> Option<&str> {
        fields
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
    };
    assert_eq!(value_of("Name"), Some("muvrix"), "rpm Name wrong\n{info}");
    assert_eq!(
        value_of("Version"),
        Some("0.1.0"),
        "rpm Version wrong\n{info}"
    );
    assert_eq!(
        value_of("Architecture"),
        Some("x86_64"),
        "rpm Architecture wrong\n{info}"
    );
}

#[test]
fn test_rpm_real_installation_in_fedora() {
    if !container_install_tests_enabled() {
        eprintln!(
            "skipping: set MUVRIX_CONTAINER_INSTALL_TESTS=1 to run container installation tests"
        );
        return;
    }
    if !have_container_engine() {
        eprintln!("skipping: no container engine available");
        return;
    }
    let root = repo_root();
    let rpm = packages_dir().join("muvrix-0.1.0-1.x86_64.rpm");
    if !rpm.exists() {
        eprintln!("skipping: {} not built", rpm.display());
        return;
    }

    let script = r#"
set -e
dnf install -y -q /packages/muvrix-0.1.0-1.x86_64.rpm
test -x /usr/bin/muvrix
rpm -q --qf '%{VERSION}\n' muvrix | grep -qx '0.1.0'
dnf remove -y -q muvrix
test ! -e /usr/bin/muvrix
echo INSTALL_UNINSTALL_OK
"#;

    let output = Command::new("podman")
        .args(["run", "--rm", "-v"])
        .arg(format!("{}/target/packages:/packages:z", root.display()))
        .args(["fedora:40", "bash", "-c", script])
        .output()
        .expect("failed to run container installation test");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success() && stdout.contains("INSTALL_UNINSTALL_OK"),
        "RPM install/uninstall failed.\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

#[test]
fn test_flatpak_bundle_is_ostree_and_imports() {
    let bundle = packages_dir().join("org.muvrix.Muvrix.flatpak");
    if !bundle.exists() {
        eprintln!("skipping: {} not built", bundle.display());
        return;
    }

    let bytes = fs::read(&bundle).expect("failed to read flatpak bundle");
    assert!(
        bytes.starts_with(b"flatpak\0"),
        "bundle is not an OSTree Flatpak (missing flatpak magic header)"
    );

    if !have("flatpak") {
        eprintln!("skipping import check: flatpak not installed");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    fs::create_dir_all(repo.join("objects")).unwrap();
    fs::create_dir_all(repo.join("refs/heads")).unwrap();
    fs::create_dir_all(repo.join("refs/mirrors")).unwrap();
    fs::create_dir_all(repo.join("refs/remotes")).unwrap();
    fs::write(
        repo.join("config"),
        "[core]\nrepo_version=1\nmode=archive-z2\n",
    )
    .unwrap();

    let output = Command::new("flatpak")
        .arg("build-import-bundle")
        .arg(&repo)
        .arg(&bundle)
        .output()
        .expect("failed to import flatpak bundle");
    assert!(
        output.status.success(),
        "flatpak rejected the bundle: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_flatpakref_descriptor_is_valid() {
    let ref_path = packages_dir().join("org.muvrix.Muvrix.flatpakref");
    if !ref_path.exists() {
        eprintln!("skipping: {} not built", ref_path.display());
        return;
    }
    let content = fs::read_to_string(&ref_path).unwrap();
    assert!(content.starts_with("[Flatpak Ref]"));
    assert!(content.contains("Name=org.muvrix.Muvrix"));
    assert!(content.contains("Branch=stable"));
    assert!(content.contains("IsRuntime=false"));
    assert!(content.contains("RuntimeRepo=https://dl.flathub.org/repo/flathub.flatpakrepo"));
}

#[test]
fn test_flatpak_real_installation() {
    if !container_install_tests_enabled() {
        eprintln!(
            "skipping: set MUVRIX_CONTAINER_INSTALL_TESTS=1 to run container installation tests"
        );
        return;
    }
    if !have("flatpak") {
        eprintln!("skipping: flatpak not installed");
        return;
    }
    let bundle = packages_dir().join("org.muvrix.Muvrix.flatpak");
    if !bundle.exists() {
        eprintln!("skipping: {} not built", bundle.display());
        return;
    }

    // Install into an isolated user installation, then uninstall.
    let temp = tempfile::tempdir().expect("tempdir");
    let user_dir = temp.path().join("flatpak-user");
    fs::create_dir_all(&user_dir).unwrap();

    let install = Command::new("flatpak")
        .args(["install", "--user", "--noninteractive", "--bundle"])
        .arg(&bundle)
        .env("FLATPAK_USER_DIR", &user_dir)
        .output()
        .expect("failed to invoke flatpak install");
    assert!(
        install.status.success(),
        "flatpak install failed: {}",
        String::from_utf8_lossy(&install.stderr)
    );

    let info = Command::new("flatpak")
        .args(["info", "--user", "org.muvrix.Muvrix"])
        .env("FLATPAK_USER_DIR", &user_dir)
        .output()
        .expect("failed to query installed flatpak");
    assert!(info.status.success());
    let info_text = String::from_utf8_lossy(&info.stdout);
    assert!(info_text.contains("org.muvrix.Muvrix"));

    let uninstall = Command::new("flatpak")
        .args(["uninstall", "--user", "-y", "org.muvrix.Muvrix"])
        .env("FLATPAK_USER_DIR", &user_dir)
        .output()
        .expect("failed to uninstall flatpak");
    assert!(uninstall.status.success());
}

#[test]
fn test_appimage_standalone_extraction_and_run() {
    let appimage_path = packages_dir().join("Muvrix-0.1.0-x86_64.AppImage");
    if !appimage_path.exists() {
        eprintln!("skipping: {} not built", appimage_path.display());
        return;
    }

    // 1. A genuine AppImage is a type-2 ELF: \x7fELF plus the "AI\x02" magic at offset 8.
    let header = std::fs::read(&appimage_path).expect("failed to read AppImage");
    assert_eq!(
        &header[0..4],
        b"\x7fELF",
        "AppImage is not an ELF executable"
    );
    assert_eq!(&header[8..11], b"AI\x02", "AppImage is not type 2");

    // 2. The embedded runtime reports its own version.
    let version_output = Command::new(&appimage_path)
        .arg("--appimage-version")
        .output()
        .expect("Failed to execute AppImage --appimage-version");
    assert!(version_output.status.success());

    // 3. Test --appimage-extract into an isolated working directory.
    let temp_dir = tempfile::tempdir().expect("Failed to create tempdir");
    let extract_dir = temp_dir.path().join("squashfs-root");

    let extract_output = Command::new(&appimage_path)
        .arg("--appimage-extract")
        .current_dir(temp_dir.path())
        .output()
        .expect("Failed to execute AppImage --appimage-extract");
    assert!(
        extract_output.status.success(),
        "extraction failed: {}",
        String::from_utf8_lossy(&extract_output.stderr)
    );

    // 3. Verify extracted AppDir layout
    assert!(extract_dir.join("AppRun").exists());
    assert!(extract_dir.join("muvrix.desktop").exists());
    assert!(extract_dir.join("org.muvrix.Muvrix.desktop").exists());
    assert!(extract_dir.join(".DirIcon").exists());
    assert!(extract_dir.join("usr/bin/muvrix").exists());
    assert!(extract_dir
        .join("usr/share/metainfo/org.muvrix.Muvrix.metainfo.xml")
        .exists());
}

#[test]
fn test_package_manifest_sha256_checksums() {
    let sums_path = packages_dir().join("SHA256SUMS");
    if !sums_path.exists() {
        eprintln!("skipping: {} not built", sums_path.display());
        return;
    }

    let output = Command::new("sha256sum")
        .args(["-c", "SHA256SUMS"])
        .current_dir(packages_dir())
        .output()
        .expect("failed to verify SHA256SUMS");
    assert!(
        output.status.success(),
        "checksum verification failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let mut info = PackagingInfo::detect();
    assert!(info.verify_checksums());
    assert!(info.manifest_verified);
}

#[test]
fn test_packaging_runtime_detector_and_portal_categories() {
    let info = PackagingInfo::detect();

    assert!(!info.portal_permissions.is_empty());
    let categories: Vec<&str> = info
        .portal_permissions
        .iter()
        .map(|p| p.category.as_str())
        .collect();
    assert!(categories.contains(&"Display & GPU"));
    assert!(categories.contains(&"Audio DSP"));
    assert!(categories.contains(&"Remote Media"));
    assert!(categories.contains(&"Filesystem"));
    assert!(categories.contains(&"IPC & Hardware Keys"));

    assert_eq!(info.release_installers.len(), 4);
    let formats: Vec<PackageFormat> = info.release_installers.iter().map(|i| i.format).collect();
    assert!(formats.contains(&PackageFormat::AppImage));
    assert!(formats.contains(&PackageFormat::Flatpak));
    assert!(formats.contains(&PackageFormat::Deb));
    assert!(formats.contains(&PackageFormat::Rpm));
}
