use clap::Parser;
use eframe::egui;
use sync2gether::app::App;

#[derive(Parser)]
#[command(version, about)]
struct Args {
    #[cfg(feature = "demo")]
    #[arg(long)]
    /// Abre cenários fictícios sem player ou conexão.
    demo: bool,
    #[cfg(feature = "demo")]
    #[arg(long, value_enum, requires = "demo", default_value = "waiting")]
    demo_state: sync2gether::demo::Scenario,
    #[cfg(feature = "demo")]
    #[arg(long, requires = "demo", value_name = "PNG")]
    /// Salva uma captura PNG da janela e encerra. Requer um display.
    demo_shot: Option<std::path::PathBuf>,
    #[cfg(feature = "demo")]
    #[arg(long, requires = "demo")]
    demo_light: bool,
}

fn main() -> eframe::Result {
    let _args = Args::parse();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([960.0, 640.0])
            .with_min_inner_size([480.0, 480.0]),
        ..Default::default()
    };
    eframe::run_native(
        "sync2gether",
        options,
        Box::new(move |cc| {
            sync2gether::ui::configure_theme(&cc.egui_ctx, false);
            #[cfg(feature = "demo")]
            if _args.demo {
                if _args.demo_light {
                    sync2gether::ui::configure_theme(&cc.egui_ctx, true);
                }
                return Ok(Box::new(App::demo(
                    _args.demo_state.into(),
                    _args.demo_shot,
                )));
            }
            Ok(Box::<App>::default())
        }),
    )
}
