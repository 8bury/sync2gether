#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use clap::Parser;
use sync2gether::app::App;
use sync2gether::diagnostics::{self, Stage};

#[derive(Parser)]
#[command(version, about)]
struct Args {
    #[arg(long)]
    diagnostics: bool,
    #[arg(long, value_name = "ARQUIVO")]
    file: Option<std::path::PathBuf>,
    #[arg(long, conflicts_with_all = ["host", "join", "code"])]
    internet: bool,
    #[arg(long, value_name = "CÓDIGO", conflicts_with_all = ["host", "join"])]
    code: Option<String>,
    #[arg(long, value_name = "NOME[:PORTA]", env = "SYNC2GETHER_RELAY", default_value = sync2gether::network::relay::DEFAULT_RELAY)]
    relay: String,
    #[arg(long, value_name = "URL", env = "SYNC2GETHER_WEB_URL")]
    web_url: Option<String>,
    #[arg(long, value_name = "IP[:PORTA]", conflicts_with = "join", num_args = 0..=1, default_missing_value = "auto")]
    host: Option<String>,
    #[arg(long, requires = "host", default_value_t = sync2gether::protocol::PORT)]
    port: u16,
    #[arg(long, value_name = "IP[:PORTA]")]
    join: Option<String>,
    #[cfg(feature = "demo")]
    #[arg(long, conflicts_with_all = ["file", "host", "join", "internet", "code"])]
    demo: bool,
    #[cfg(feature = "demo")]
    #[arg(long, value_enum, requires = "demo", default_value = "waiting")]
    demo_state: sync2gether::demo::Scenario,
    #[cfg(feature = "demo")]
    #[arg(long, requires = "demo", value_name = "PNG")]
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
            if _args.internet {
                commands.push(sync2gether::runtime::Command::HostInternet(
                    _args.relay.clone(),
                ));
            }
            if let Some(code) = _args.code {
                commands.push(sync2gether::runtime::Command::JoinCode {
                    relay: _args.relay.clone(),
                    code,
                });
            }
            Ok(Box::new(
                App::with_commands_gl(cc, commands)?
                    .with_relay(_args.relay)
                    .with_web_url(_args.web_url),
            ))
        }),
    );
    diagnostics::mark(if result.is_ok() {
        Stage::Exit
    } else {
        Stage::NativeError
    });
    result
}
