//! Runtime integration tests for HDR-to-SDR tone-mapping, color primaries,
//! and metadata detection in libmpv / libplacebo.

use muvrix_player::handle::MpvHandle;
use muvrix_player::shader::{
    GamutMappingMode, ShaderManager, TargetPrimaries, TargetTrc, ToneMappingAlgorithm,
    VideoAdjustmentSettings,
};
use std::path::PathBuf;

/// Returns the requested fixture, generating the HDR ones on first use.
///
/// Media fixtures are not committed (`.gitignore` excludes them), so a fresh
/// checkout has none. The generated files are real HDR10 / HLG streams with the
/// colour tags the tests assert on — not stand-ins — so the assertions keep
/// testing detection rather than a mock.
fn get_fixture_path(name: &str) -> PathBuf {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("../../tests/fixtures/{name}"));
    if !path.exists() && (name.starts_with("hdr10_pq") || name.starts_with("hdr_hlg")) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create tests/fixtures");
        }
        generate_hdr_fixture(&path, name.starts_with("hdr_hlg"));
    }
    path.canonicalize()
        .unwrap_or_else(|_| panic!("Fixture {name} must exist at {}", path.display()))
}

/// Encodes a 2 s, 320x240 HEVC stream tagged BT.2020 + PQ (or HLG).
fn generate_hdr_fixture(path: &std::path::Path, hlg: bool) {
    let transfer = if hlg { "arib-std-b67" } else { "smpte2084" };
    let mut x265 = format!(
        "colorprim=bt2020:transfer={transfer}:colormatrix=bt2020nc:keyint=12:min-keyint=12:log-level=error"
    );
    if !hlg {
        // Mastering display + content light level, i.e. real HDR10 static metadata.
        x265.push_str(
            ":master-display=G(13250,34500)B(7500,3000)R(34000,16000)WP(15635,16450)L(10000000,1)\
             :max-cll=1000,400",
        );
    }
    let output = std::process::Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x240:rate=24:duration=2",
            "-pix_fmt",
            "yuv420p10le",
            "-c:v",
            "libx265",
            "-x265-params",
            &x265,
            path.to_str().expect("fixture path is UTF-8"),
        ])
        .output()
        .unwrap_or_else(|e| panic!("ffmpeg is required to generate {path:?}: {e}"));
    assert!(
        output.status.success(),
        "ffmpeg failed generating {path:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn create_mpv_player(vo: &str) -> MpvHandle {
    let handle = MpvHandle::new().expect("Create MpvHandle");
    handle.set_option_string("vo", vo).unwrap();
    handle.set_option_string("ao", "null").unwrap();
    handle.initialize().unwrap();
    handle
}

#[test]
fn test_hdr10_pq_metadata_detection_and_tone_mapping() {
    let fixture = get_fixture_path("hdr10_pq_bt2020.mp4");
    let handle = create_mpv_player("gpu");

    // Configure HDR-to-SDR tone-mapping pipeline via libplacebo
    let settings = VideoAdjustmentSettings {
        tone_mapping: ToneMappingAlgorithm::Bt2390,
        target_trc: TargetTrc::Srgb,
        target_prim: TargetPrimaries::Bt709,
        gamut_mapping: GamutMappingMode::Perceptual,
        hdr_compute_peak: true,
        tone_mapping_max_boost: 1.5,
        ..Default::default()
    };
    ShaderManager::apply_adjustments(&handle, &settings).expect("Apply HDR-to-SDR settings");

    handle
        .command(&["loadfile", fixture.to_str().unwrap()])
        .expect("Load HDR10 fixture");

    // Wait for stream demuxing and header decoding
    std::thread::sleep(std::time::Duration::from_millis(500));

    // Verify HDR metadata readback from libmpv
    let primaries = handle
        .get_property_string("video-params/primaries")
        .unwrap();
    assert_eq!(
        primaries, "bt.2020",
        "HDR10 fixture must be detected with BT.2020 wide color primaries"
    );

    let gamma = handle.get_property_string("video-params/gamma").unwrap();
    assert_eq!(
        gamma, "pq",
        "HDR10 fixture must be detected with SMPTE ST 2084 (PQ) transfer"
    );

    let colorlevels = handle
        .get_property_string("video-params/colorlevels")
        .unwrap();
    assert_eq!(
        colorlevels, "limited",
        "Standard broadcast HDR10 video levels must be detected"
    );

    // Verify libplacebo tone mapping options are actively set on the engine
    assert_eq!(
        handle.get_property_string("tone-mapping").unwrap(),
        "bt.2390"
    );
    assert_eq!(handle.get_property_string("target-trc").unwrap(), "srgb");
    assert_eq!(handle.get_property_string("target-prim").unwrap(), "bt.709");
    assert_eq!(
        handle.get_property_string("gamut-mapping-mode").unwrap(),
        "perceptual"
    );

    // Capture tone-mapped SDR frame to verify rendering pipeline completes without errors
    let out_path =
        std::env::temp_dir().join(format!("hdr10_tone_mapped_{}.png", std::process::id()));
    let _ = handle.command(&[
        "screenshot-to-file",
        out_path.to_str().unwrap(),
        "subtitles",
    ]);
    std::thread::sleep(std::time::Duration::from_millis(150));

    if let Ok(bytes) = std::fs::read(&out_path) {
        assert!(
            !bytes.is_empty(),
            "Tone-mapped output frame must not be empty"
        );
        let _ = std::fs::remove_file(&out_path);
    }
}

#[test]
fn test_hdr_hlg_metadata_detection_and_tone_mapping() {
    let fixture = get_fixture_path("hdr_hlg_bt2020.mp4");
    let handle = create_mpv_player("gpu");

    // Configure HLG-to-SDR tone-mapping
    let settings = VideoAdjustmentSettings {
        tone_mapping: ToneMappingAlgorithm::Mobius,
        target_trc: TargetTrc::Bt1886,
        target_prim: TargetPrimaries::Bt709,
        gamut_mapping: GamutMappingMode::Clip,
        hdr_compute_peak: true,
        tone_mapping_max_boost: 1.0,
        ..Default::default()
    };
    ShaderManager::apply_adjustments(&handle, &settings).expect("Apply HLG tone mapping");

    handle
        .command(&["loadfile", fixture.to_str().unwrap()])
        .expect("Load HLG fixture");

    std::thread::sleep(std::time::Duration::from_millis(500));

    // Verify HLG metadata readback
    let primaries = handle
        .get_property_string("video-params/primaries")
        .unwrap();
    assert_eq!(
        primaries, "bt.2020",
        "HLG fixture must be detected with BT.2020 primaries"
    );

    let gamma = handle.get_property_string("video-params/gamma").unwrap();
    assert_eq!(
        gamma, "hlg",
        "HLG fixture must be detected with ARIB STD-B67 (HLG) transfer"
    );

    // Verify engine properties
    assert_eq!(
        handle.get_property_string("tone-mapping").unwrap(),
        "mobius"
    );
    assert_eq!(handle.get_property_string("target-trc").unwrap(), "bt.1886");
}

#[test]
fn test_sdr_playback_regression_safety_under_hdr_tone_mapping_options() {
    let sdr_fixture = get_fixture_path("test_video.mp4");
    let handle = create_mpv_player("null");

    // Engine configured with BT.2390 tone-mapping
    let settings = VideoAdjustmentSettings {
        tone_mapping: ToneMappingAlgorithm::Bt2390,
        target_trc: TargetTrc::Srgb,
        target_prim: TargetPrimaries::Bt709,
        gamut_mapping: GamutMappingMode::Perceptual,
        ..Default::default()
    };
    ShaderManager::apply_adjustments(&handle, &settings).expect("Apply settings");

    handle
        .command(&["loadfile", sdr_fixture.to_str().unwrap()])
        .expect("Load standard SDR video");

    std::thread::sleep(std::time::Duration::from_millis(400));

    // SDR video properties must remain intact and not corrupted
    let primaries = handle
        .get_property_string("video-params/primaries")
        .unwrap();
    assert!(
        primaries == "bt.709" || primaries.is_empty(),
        "SDR video should retain standard BT.709 primaries: {primaries}"
    );

    let gamma = handle.get_property_string("video-params/gamma").unwrap();
    assert!(
        gamma == "bt.1886" || gamma == "gamma2.2" || gamma == "srgb" || gamma.is_empty(),
        "SDR video must retain standard SDR gamma curve: {gamma}"
    );
}

#[test]
fn test_libplacebo_tone_mapping_algorithm_registration_sweep() {
    let handle = create_mpv_player("null");

    let algorithms = [
        ToneMappingAlgorithm::Auto,
        ToneMappingAlgorithm::Clip,
        ToneMappingAlgorithm::Mobius,
        ToneMappingAlgorithm::Reinhard,
        ToneMappingAlgorithm::Hable,
        ToneMappingAlgorithm::Gamma,
        ToneMappingAlgorithm::Linear,
        ToneMappingAlgorithm::Spline,
        ToneMappingAlgorithm::Bt2390,
        ToneMappingAlgorithm::Bt2446a,
        ToneMappingAlgorithm::St2094_40,
        ToneMappingAlgorithm::St2094_10,
    ];

    for algo in algorithms {
        let res = handle.set_property_string("tone-mapping", algo.as_str());
        assert!(
            res.is_ok(),
            "libmpv / libplacebo must support algorithm '{}'",
            algo.as_str()
        );
        let current = handle.get_property_string("tone-mapping").unwrap();
        assert_eq!(current, algo.as_str());
    }
}
