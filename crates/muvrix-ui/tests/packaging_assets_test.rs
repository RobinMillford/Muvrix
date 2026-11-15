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
}
