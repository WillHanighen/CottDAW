//! CottDAW main application.

mod app;
mod audio;
mod plugins;
mod ui;

use anyhow::Result;
use tracing_subscriber::EnvFilter;

fn main() -> Result<()> {
    // Prefer X11 for VST3 embedding on Linux.
    if std::env::var_os("WINIT_UNIX_BACKEND").is_none() {
        // SAFETY: called before any threads spawn.
        unsafe {
            std::env::set_var("WINIT_UNIX_BACKEND", "x11");
        }
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env()
                .add_directive("cott_daw=info".parse()?)
                .add_directive("cott_vst_worker=info".parse()?),
        )
        .init();

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 900.0])
            .with_title("CottDAW"),
        ..Default::default()
    };

    eframe::run_native(
        "CottDAW",
        native_options,
        Box::new(|cc| Ok(Box::new(app::CottApp::new(cc)))),
    )
    .map_err(|e| anyhow::anyhow!("eframe: {e}"))?;
    Ok(())
}
