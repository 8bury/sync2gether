//! Executa o relay. Endereço por `--listen ENDEREÇO` ou `RELAY_LISTEN`.
use std::net::SocketAddr;

fn listen_address() -> Result<SocketAddr, String> {
    let mut args = std::env::args().skip(1);
    let mut listen = std::env::var("RELAY_LISTEN").ok();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--listen" => listen = Some(args.next().ok_or("--listen requer um endereço")?),
            "--version" => {
                println!("sync2gether-relay {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "--help" | "-h" => {
                println!(
                    "Uso: sync2gether-relay [--listen ENDEREÇO:PORTA]\n\
                     Padrão: 0.0.0.0:{}. Também aceita RELAY_LISTEN.",
                    sync2gether_relay::PORT
                );
                std::process::exit(0);
            }
            other => return Err(format!("Argumento desconhecido: {other}")),
        }
    }
    listen
        .unwrap_or_else(|| format!("0.0.0.0:{}", sync2gether_relay::PORT))
        .parse()
        .map_err(|_| "Endereço inválido; use IP:PORTA".into())
}

#[tokio::main]
async fn main() {
    let address = match listen_address() {
        Ok(address) => address,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };
    let listener = match tokio::net::TcpListener::bind(address).await {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("Não foi possível escutar em {address}: {error}");
            std::process::exit(1);
        }
    };
    eprintln!(
        "sync2gether-relay {} escutando em {address}, protocolo {}",
        env!("CARGO_PKG_VERSION"),
        sync2gether_relay::VERSION
    );
    let serve = sync2gether_relay::serve_with_stats(
        listener,
        sync2gether_relay::Limits::default(),
        |stats| {
            eprintln!(
                "conexões={} salas={} pareadas={}",
                stats.connections, stats.rooms, stats.pipes
            )
        },
    );
    let mut terminate =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).ok();
    tokio::select! {
        _ = serve => {}
        _ = tokio::signal::ctrl_c() => {}
        _ = async { match &mut terminate { Some(signal) => { signal.recv().await; } None => std::future::pending().await } } => {}
    }
    eprintln!("Encerrando o relay");
}
