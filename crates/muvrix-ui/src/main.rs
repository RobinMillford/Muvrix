//! Muvrix desktop entrypoint launching the eframe OpenGL application.

use eframe::egui;
use muvrix_ui::MuvrixApp;

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let mut initial_file: Option<String> = None;

    for arg in args.iter().skip(1) {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("Muvrix Desktop Media Player");
                println!("Usage: muvrix-ui [OPTIONS] [MEDIA_FILE]");
                println!();
                println!("Arguments:");
                println!(
                    "  [MEDIA_FILE]   Local media path (relative, absolute, ~/) or network URL"
                );
                println!();
                println!("Options:");
                println!("  -h, --help     Print help information and exit");
                println!("  -v, --version  Print version information and exit");
                return Ok(());
            }
            "-v" | "--version" => {
                println!("muvrix-ui {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            other if !other.starts_with('-') => {
                if initial_file.is_none() {
                    initial_file = Some(other.to_string());
                }
            }
            unknown_flag => {
                eprintln!("Unknown option: '{unknown_flag}'");
                eprintln!("Run 'muvrix-ui --help' for usage instructions.");
                std::process::exit(1);
            }
        }
    }

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    log::info!("Starting Muvrix Desktop Player");

    // Native presentation path (Phase 2.0). eframe remains available as an
    // explicit rollback: MUVRIX_LEGACY_EFRAME=1 restores the old shell, which is
    // the only way to compare the two on the same machine.
    #[cfg(target_os = "linux")]
    if std::env::var("MUVRIX_LEGACY_EFRAME")
        .map(|v| v != "1")
        .unwrap_or(true)
    {
        let width = std::env::var("MUVRIX_WINDOW_WIDTH")
            .ok()
            .and_then(|w| w.parse::<u32>().ok())
            .unwrap_or(1280);
        let height = std::env::var("MUVRIX_WINDOW_HEIGHT")
            .ok()
            .and_then(|h| h.parse::<u32>().ok())
            .unwrap_or(720);
        let cfg = muvrix_ui::runner::RunnerConfig {
            title: "Muvrix Media Player".to_string(),
            width,
            height,
            initial_file,
            want_hdr: std::env::var("MUVRIX_NO_HDR")
                .map(|v| v != "1")
                .unwrap_or(true),
        };
        return match muvrix_ui::runner::run(cfg) {
            Ok(()) => Ok(()),
            Err(e) => {
                log::error!("native presentation runner failed: {e}");
                Err(eframe::Error::AppCreation(e.into()))
            }
        };
    }

    let width = std::env::var("MUVRIX_WINDOW_WIDTH")
        .ok()
        .and_then(|w| w.parse::<f32>().ok())
        .unwrap_or(1280.0);
    let height = std::env::var("MUVRIX_WINDOW_HEIGHT")
        .ok()
        .and_then(|h| h.parse::<f32>().ok())
        .unwrap_or(720.0);

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Muvrix Media Player (Glow Prototype)")
            .with_inner_size([width, height])
            .with_min_inner_size([640.0, 360.0]),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };

    eframe::run_native(
        "Muvrix Media Player",
        native_options,
        Box::new(move |cc| {
            let mut app = MuvrixApp::new(cc).expect("Failed to initialize MuvrixApp");
            if let Some(path) = initial_file {
                app.load_file(&path);
            }
            Ok(Box::new(app))
        }),
    )
}
