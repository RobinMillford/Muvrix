use std::fs;

fn repo_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

#[test]
fn desktop_entry_and_launcher_rpath_guidance_are_valid() {
    let root = repo_root();
    let desktop = fs::read_to_string(root.join("packaging/muvrix.desktop")).expect("desktop");
    assert!(desktop.contains("Type=Application"));
    assert!(desktop.contains("Exec=muvrix %F"));
    for mime in [
        "video/mp4",
        "video/x-matroska",
        "video/webm",
        "video/x-msvideo",
        "video/quicktime",
    ] {
        assert!(desktop.contains(mime), "missing MIME type {mime}");
    }
    let launcher = fs::read_to_string(root.join("packaging/muvrix")).expect("launcher");
    assert!(launcher.contains("LD_LIBRARY_PATH"));
    assert!(launcher.contains("lib64"));
    assert!(launcher.contains("muvrix-bin"));
    assert!(launcher.contains("MUVRIX_ASSETS_DIR"));

    // Branding and packaging icon verification
    assert!(desktop.contains("Icon=io.muvrix.Muvrix"));
    let svg = root.join("assets/branding/muvrix-logo.svg");
    let png = root.join("assets/branding/muvrix-logo.png");
    let ico = root.join("assets/branding/muvrix.ico");
    let icns = root.join("assets/branding/muvrix.icns");
    assert!(svg.is_file() && fs::metadata(&svg).unwrap().len() > 0);
    assert!(png.is_file() && fs::metadata(&png).unwrap().len() > 0);
    assert!(ico.is_file() && fs::metadata(&ico).unwrap().len() > 0);
    assert!(icns.is_file() && fs::metadata(&icns).unwrap().len() > 0);

    for size in [16, 24, 32, 48, 64, 128, 256, 512] {
        let icon_path = root.join(format!(
            "assets/branding/icons/muvrix-{}x{}.png",
            size, size
        ));
        assert!(
            icon_path.is_file() && fs::metadata(&icon_path).unwrap().len() > 0,
            "missing or empty icon: {icon_path:?}"
        );
    }
}
