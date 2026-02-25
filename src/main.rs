mod app;
mod nix;
mod util;

use clap::Parser;

#[derive(Debug, Parser)]
#[command(author, version, about)]
struct Args {
    #[arg(long, default_value = "/run/current-system")]
    system_path: String,
}

fn main() -> eframe::Result<()> {
    let args = Args::parse();
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_inner_size([1440.0, 920.0]),
        ..Default::default()
    };

    eframe::run_native(
        "nix-analisá",
        options,
        Box::new(move |cc| {
            Ok(Box::new(app::NixAnalyzeApp::new(
                cc,
                args.system_path.clone(),
            )))
        }),
    )
}
