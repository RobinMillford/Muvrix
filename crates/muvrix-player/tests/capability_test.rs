use std::path::PathBuf;
use std::time::Duration;

use muvrix_core::platform::PlatformInfo;
use muvrix_player::{HdrPresentationMode, HwdecMemoryPath, HwdecStatus, LoadFileMode, MpvHandle};

fn get_test_video_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/test_video.mp4")
        .canonicalize()
        .expect("Tracked test fixture 'tests/fixtures/test_video.mp4' must exist")
}

fn find_corpus_media_path(relative: &str) -> Option<PathBuf> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../")
        .join(relative);
    if path.exists() {
        Some(path.canonicalize().unwrap_or(path))
    } else {
        None
    }
}

fn create_player(hwdec: &str) -> MpvHandle {
    let handle = MpvHandle::new().expect("Failed to create MpvHandle");
    handle.set_option_string("vo", "null").unwrap();
    handle.set_option_string("ao", "null").unwrap();
    handle.set_option_string("idle", "yes").unwrap();
    handle.set_option_string("hwdec", hwdec).unwrap();
    handle.initialize().unwrap();
    handle
}

#[test]
fn test_distro_parsing_across_distributions() {
    // Fedora 45
    let fedora_content = r#"
NAME="Fedora Linux"
VERSION="45 (Workstation Edition)"
ID=fedora
VERSION_ID=45
PRETTY_NAME="Fedora Linux 45 (Workstation Edition)"
"#;
    let fedora = PlatformInfo::parse_os_release_content(fedora_content);
    assert_eq!(fedora.id, "fedora");
    assert_eq!(fedora.name, "Fedora Linux");
    assert_eq!(fedora.version_id, "45");

    // Ubuntu 24.04 LTS
    let ubuntu_content = r#"
NAME="Ubuntu"
VERSION="24.04 LTS (Noble Numbat)"
ID=ubuntu
VERSION_ID="24.04"
PRETTY_NAME="Ubuntu 24.04 LTS"
"#;
    let ubuntu = PlatformInfo::parse_os_release_content(ubuntu_content);
    assert_eq!(ubuntu.id, "ubuntu");
    assert_eq!(ubuntu.version_id, "24.04");

    // Debian 12 (Bookworm)
    let debian_content = r#"
NAME="Debian GNU/Linux"
VERSION_ID="12"
VERSION="12 (bookworm)"
ID=debian
PRETTY_NAME="Debian GNU/Linux 12 (bookworm)"
"#;
    let debian = PlatformInfo::parse_os_release_content(debian_content);
    assert_eq!(debian.id, "debian");
    assert_eq!(debian.version_id, "12");

    // Arch Linux
    let arch_content = r#"
NAME="Arch Linux"
PRETTY_NAME="Arch Linux"
ID=arch
"#;
    let arch = PlatformInfo::parse_os_release_content(arch_content);
    assert_eq!(arch.id, "arch");
    assert_eq!(arch.version_id, "");

    // openSUSE Tumbleweed
    let suse_content = r#"
NAME="openSUSE Tumbleweed"
ID="opensuse-tumbleweed"
ID_LIKE="opensuse suse"
VERSION_ID="20261101"
PRETTY_NAME="openSUSE Tumbleweed"
"#;
    let suse = PlatformInfo::parse_os_release_content(suse_content);
    assert_eq!(suse.id, "opensuse-tumbleweed");

    // NixOS
    let nixos_content = r#"
NAME=NixOS
ID=nixos
VERSION="24.11"
VERSION_ID="24.11"
PRETTY_NAME="NixOS 24.11"
"#;
    let nixos = PlatformInfo::parse_os_release_content(nixos_content);
    assert_eq!(nixos.id, "nixos");
    assert_eq!(nixos.version_id, "24.11");
}

#[test]
fn test_engine_capabilities_detection() {
    let player = create_player("auto-safe");
    let caps = player.detect_engine_capabilities();

    println!("Detected Engine Capabilities:");
    println!(" - OS:              {}", caps.platform.os);
    println!(" - Distro:          {}", caps.platform.distro.pretty_name);
    println!(" - Kernel:          {}", caps.platform.kernel_version);
    println!(
        " - Session:         {}",
        caps.platform.session_type.as_str()
    );
    println!(" - DRM Render Node: {:?}", caps.platform.drm_render_node);
    println!(" - DRM Accessible:  {}", caps.platform.drm_accessible);
    println!(" - libmpv API:      {}", caps.api_version);
    println!(" - mpv Version:     {}", caps.mpv_version);
    println!(" - FFmpeg Version:  {}", caps.ffmpeg_version);

    assert_eq!(caps.platform.os, std::env::consts::OS);
    #[cfg(target_os = "linux")]
    {
        assert!(caps.platform.kernel_version != "Unknown");
    }
    assert!(caps.api_version.is_gl_compatible());
    assert!(!caps.mpv_version.is_empty());
    assert!(!caps.ffmpeg_version.is_empty());
}

#[test]
#[ignore = "Requires physical GPU hardware decoder (e.g. VA-API /dev/dri/renderD128, VideoToolbox, D3D11VA); run with: cargo test -p muvrix-player --test capability_test -- test_hwdec_active_copyback --ignored --nocapture"]
fn test_hwdec_active_copyback() {
    let path = get_test_video_fixture();
    let player = create_player("auto-safe");

    player
        .load_file(path.to_str().unwrap(), LoadFileMode::Replace)
        .unwrap();
    std::thread::sleep(Duration::from_millis(200));

    let diag = player.query_playback_diagnostics("auto-safe").unwrap();
    println!("HWDEC Active Diagnostic: {:?}", diag.hwdec_status);

    match diag.hwdec_status {
        HwdecStatus::Active {
            ref decoder,
            memory_path,
        } => {
            println!("Active decoder: {decoder}");
            assert_eq!(memory_path, HwdecMemoryPath::CopyBack);
            assert!(
                decoder.contains("vaapi")
                    || decoder.contains("videotoolbox")
                    || decoder.contains("d3d11va")
            );
        }
        other => panic!("Expected HwdecStatus::Active, got: {:?}", other),
    }

    assert_eq!(diag.hdr_mode, HdrPresentationMode::SdrOnly);
}

#[test]
fn test_hwdec_software_fallback() {
    // Requesting nvdec on non-NVIDIA hardware gracefully falls back to software decoding
    let path = get_test_video_fixture();
    let player = create_player("nvdec");

    player
        .load_file(path.to_str().unwrap(), LoadFileMode::Replace)
        .unwrap();
    std::thread::sleep(Duration::from_millis(200));

    let diag = player.query_playback_diagnostics("nvdec").unwrap();
    println!("HWDEC Fallback Diagnostic: {:?}", diag.hwdec_status);

    match diag.hwdec_status {
        HwdecStatus::SoftwareFallback {
            ref requested,
            ref active_decoder,
            ..
        } => {
            assert_eq!(requested, "nvdec");
            assert_eq!(active_decoder, "h264");
        }
        other => panic!("Expected HwdecStatus::SoftwareFallback, got: {:?}", other),
    }
}

#[test]
fn test_hwdec_explicitly_disabled() {
    let path = get_test_video_fixture();
    let player = create_player("no");

    player
        .load_file(path.to_str().unwrap(), LoadFileMode::Replace)
        .unwrap();
    std::thread::sleep(Duration::from_millis(200));

    let diag = player.query_playback_diagnostics("no").unwrap();
    assert_eq!(diag.hwdec_status, HwdecStatus::Disabled);
}

#[test]
fn test_sdr_detection_and_presentation_mode() {
    let path = get_test_video_fixture();
    let player = create_player("auto-safe");

    player
        .load_file(path.to_str().unwrap(), LoadFileMode::Replace)
        .unwrap();
    std::thread::sleep(Duration::from_millis(200));

    let diag = player.query_playback_diagnostics("auto-safe").unwrap();
    println!("SDR Diagnostic: {:?}", diag.hdr_mode);
    assert_eq!(diag.hdr_mode, HdrPresentationMode::SdrOnly);
}

#[test]
#[ignore = "Requires HDR test media from extended corpus (hevc-main10-hdr10); run with: cargo test -p muvrix-player --test capability_test -- test_hdr_detection_and_tonemapping_status --ignored --nocapture"]
fn test_hdr_detection_and_tonemapping_status() {
    let path = find_corpus_media_path(
        "tests/MuvrixTestMedia/phase-0.4/generated/hevc-main10-hdr10-synthetic.mp4",
    )
    .or_else(|| {
        find_corpus_media_path("tests/MuvrixTestMedia/phase-0.4/A-core/hevc-main10-hdr10.mp4")
    })
    .expect("HDR test fixture missing from tests/MuvrixTestMedia/");

    let player = create_player("auto-safe");
    player
        .load_file(path.to_str().unwrap(), LoadFileMode::Replace)
        .unwrap();
    std::thread::sleep(Duration::from_millis(200));

    let diag = player.query_playback_diagnostics("auto-safe").unwrap();
    println!("HDR Diagnostic: {:?}", diag.hdr_mode);

    match diag.hdr_mode {
        HdrPresentationMode::ToneMappedToSdr {
            ref transfer,
            ref primaries,
        } => {
            println!("HDR Transfer: {transfer}, Primaries: {primaries}");
            assert!(
                transfer.contains("pq")
                    || transfer.contains("smpte2084")
                    || primaries.contains("2020")
            );
        }
        other => panic!(
            "Expected ToneMappedToSdr for HDR10 sample, got: {:?}",
            other
        ),
    }
}
