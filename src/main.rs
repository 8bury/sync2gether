use clap::Parser;
use sync2gether::app::App;
use sync2gether::diagnostics::{self, Stage};

#[derive(Parser)]
#[command(version, about)]
struct Args {
    #[arg(long)]
    /// Grava checkpoints de diagnóstico em .cache/, sem dados da mídia ou sala.
    diagnostics: bool,
    #[arg(long, value_name = "ARQUIVO")]
    /// Abre uma cópia local, inicialmente pausada.
    file: Option<std::path::PathBuf>,
    #[arg(long, value_name = "IP[:PORTA]", conflicts_with = "join", num_args = 0..=1, default_missing_value = "auto")]
    /// Hospeda automaticamente na rede, ou no endereço informado.
    host: Option<String>,
    #[arg(long, requires = "host", default_value_t = sync2gether::protocol::PORT)]
    /// Porta para hospedagem automática.
    port: u16,
    #[arg(long, value_name = "IP[:PORTA]")]
    /// Entra na sala do anfitrião.
    join: Option<String>,
    #[cfg(feature = "demo")]
    #[arg(long, conflicts_with_all = ["file", "host", "join"])]
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
    let _diagnostics = if _args.diagnostics {
        let (guard, path) = sync2gether::diagnostics::start()
            .map_err(|error| eframe::Error::AppCreation(Box::new(error)))?;
        eprintln!("Diagnóstico: {}", path.display());
        Some(guard)
    } else {
        None
    };
    let options = sync2gether::window::options();
    let title = if _args.diagnostics {
        "sync2gether [diagnóstico]"
    } else {
        "sync2gether"
    };
    diagnostics::mark(Stage::NativeLoop);
    let result = eframe::run_native(
        title,
        options,
        Box::new(move |cc| {
            diagnostics::mark(Stage::CreatingApp);
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
            let mut commands = Vec::new();
            if let Some(path) = _args.file {
                commands.push(sync2gether::runtime::Command::OpenPath(path));
            }
            if let Some(address) = _args.host {
                commands.push(if address == "auto" {
                    sync2gether::runtime::Command::HostAuto(_args.port)
                } else {
                    sync2gether::runtime::Command::HostRoom(address)
                });
            }
            if let Some(address) = _args.join {
                commands.push(sync2gether::runtime::Command::RequestJoin(address));
            }
            Ok(Box::new(App::with_commands_gl(cc, commands)?))
        }),
    );
    diagnostics::mark(if result.is_ok() {
        Stage::Exit
    } else {
        Stage::NativeError
    });
    result
}
