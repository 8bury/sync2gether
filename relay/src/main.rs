use std::{net::SocketAddr, path::PathBuf};
use sync2gether_relay::{Limits, Relay, ws};

#[derive(Debug, PartialEq, Eq)]
struct Config {
    listen: SocketAddr,
    tls: Option<(PathBuf, PathBuf)>,
    web: Option<(SocketAddr, ws::Config)>,
}

#[derive(Debug, PartialEq, Eq)]
enum Action {
    Run(Config),
    Print(String),
}

#[derive(Default)]
struct Env {
    listen: Option<String>,
    tls_cert: Option<String>,
    tls_key: Option<String>,
    ws_listen: Option<String>,
    ws_origins: Option<String>,
    ws_client_ip_header: Option<String>,
}
impl Env {
    fn read() -> Self {
        let var = |name: &str| std::env::var(name).ok();
        Self {
            listen: var("RELAY_LISTEN"),
            tls_cert: var("RELAY_TLS_CERT"),
            tls_key: var("RELAY_TLS_KEY"),
            ws_listen: var("RELAY_WS_LISTEN"),
            ws_origins: var("RELAY_WS_ORIGINS"),
            ws_client_ip_header: var("RELAY_WS_CLIENT_IP_HEADER"),
        }
    }
}

fn present(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn address(value: &str) -> Result<SocketAddr, String> {
    value
        .parse()
        .map_err(|_| "Endereço inválido; use IP:PORTA".to_owned())
}

fn parse_config(args: impl IntoIterator<Item = String>, env: Env) -> Result<Action, String> {
    let mut args = args.into_iter();
    let mut listen = present(env.listen);
    let mut tls_cert = present(env.tls_cert).map(PathBuf::from);
    let mut tls_key = present(env.tls_key).map(PathBuf::from);
    let mut ws_listen = present(env.ws_listen);
    let mut origins: Vec<String> = present(env.ws_origins)
        .map(|list| {
            list.split(',')
                .map(str::trim)
                .filter(|origin| !origin.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let mut client_ip_header = present(env.ws_client_ip_header);
    while let Some(arg) = args.next() {
        let mut value = |message: &str| args.next().ok_or_else(|| message.to_owned());
        match arg.as_str() {
            "--listen" => listen = Some(value("--listen requer um endereço")?),
            "--tls-cert" => tls_cert = Some(PathBuf::from(value("--tls-cert requer um caminho")?)),
            "--tls-key" => tls_key = Some(PathBuf::from(value("--tls-key requer um caminho")?)),
            "--ws-listen" => ws_listen = Some(value("--ws-listen requer um endereço")?),
            "--ws-origin" => origins.push(value("--ws-origin requer uma origem")?),
            "--ws-client-ip-header" => {
                client_ip_header = Some(value("--ws-client-ip-header requer um nome")?)
            }
            "--version" => {
                return Ok(Action::Print(format!(
                    "sync2gether-relay {}",
                    env!("CARGO_PKG_VERSION")
                )));
            }
            "--help" | "-h" => {
                return Ok(Action::Print(format!(
                    "Uso: sync2gether-relay [--listen ENDEREÇO:PORTA] [--tls-cert ARQUIVO --tls-key ARQUIVO]\n\
                     \x20                        [--ws-listen ENDEREÇO:PORTA] [--ws-origin ORIGEM]... [--ws-client-ip-header NOME]\n\
                     Padrão: 0.0.0.0:{}. Também aceita RELAY_LISTEN, RELAY_TLS_CERT, RELAY_TLS_KEY,\n\
                     RELAY_WS_LISTEN, RELAY_WS_ORIGINS (separadas por vírgula) e RELAY_WS_CLIENT_IP_HEADER.",
                    sync2gether_relay::PORT
                )));
            }
            other => return Err(format!("Argumento desconhecido: {other}")),
        }
    }
    let listen =
        address(&listen.unwrap_or_else(|| format!("0.0.0.0:{}", sync2gether_relay::PORT)))?;
    let tls = sync2gether_relay::tls::resolve_paths(tls_cert, tls_key)?;
    let web = match ws_listen {
        Some(ws_listen) => Some((
            address(&ws_listen)?,
            ws::Config {
                origins,
                client_ip_header: client_ip_header.map(|name| name.to_ascii_lowercase()),
            },
        )),
        None if !origins.is_empty() || client_ip_header.is_some() => {
            return Err("As opções de WebSocket exigem --ws-listen (ou RELAY_WS_LISTEN).".into());
        }
        None => None,
    };
    Ok(Action::Run(Config { listen, tls, web }))
}

#[tokio::main]
async fn main() {
    let config = match parse_config(std::env::args().skip(1), Env::read()) {
        Ok(Action::Run(config)) => config,
        Ok(Action::Print(text)) => {
            println!("{text}");
            return;
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };
    let acceptor = match &config.tls {
        Some((cert, key)) => match sync2gether_relay::tls::load_acceptor(cert, key) {
            Ok(acceptor) => Some(acceptor),
            Err(error) => {
                eprintln!("Não foi possível configurar o TLS: {error}");
                std::process::exit(1);
            }
        },
        None => None,
    };
    let bind = |address: SocketAddr| async move {
        match tokio::net::TcpListener::bind(address).await {
            Ok(listener) => listener,
            Err(error) => {
                eprintln!("Não foi possível escutar em {address}: {error}");
                std::process::exit(1);
            }
        }
    };
    let listener = bind(config.listen).await;
    let web = match config.web {
        Some((address, ws_config)) => Some((bind(address).await, address, ws_config)),
        None => None,
    };
    let mode = if acceptor.is_some() { "TLS" } else { "TCP" };
    eprintln!(
        "sync2gether-relay {} escutando em {} ({mode}), protocolo {}",
        env!("CARGO_PKG_VERSION"),
        config.listen,
        sync2gether_relay::VERSION
    );
    if let Some((_, address, _)) = &web {
        eprintln!("WebSocket para navegadores em {address}{}", ws::PATH);
    }
    let relay = Relay::new(Limits::default(), acceptor);
    let report = relay.report(|stats: sync2gether_relay::Stats| {
        eprintln!(
            "conexões={} salas={} pareadas={}",
            stats.connections, stats.rooms, stats.pipes
        )
    });
    let browsers = async {
        match web {
            Some((listener, _, ws_config)) => relay.serve_ws(listener, ws_config).await,
            None => std::future::pending().await,
        }
    };
    tokio::select! {
        _ = relay.serve(listener) => {}
        _ = browsers => {}
        _ = report => {}
        _ = tokio::signal::ctrl_c() => {}
        _ = terminate() => {}
    }
    eprintln!("Encerrando o relay");
}

#[cfg(unix)]
async fn terminate() {
    match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
        Ok(mut signal) => {
            signal.recv().await;
        }
        Err(_) => std::future::pending().await,
    }
}

#[cfg(not(unix))]
async fn terminate() {
    std::future::pending().await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn default_listen() -> SocketAddr {
        format!("0.0.0.0:{}", sync2gether_relay::PORT)
            .parse()
            .unwrap()
    }

    fn run(listen: SocketAddr, tls: Option<(&str, &str)>) -> Action {
        Action::Run(Config {
            listen,
            tls: tls.map(|(c, k)| (PathBuf::from(c), PathBuf::from(k))),
            web: None,
        })
    }

    #[test]
    fn parses_plain_and_tls_options_from_flags_and_env() {
        assert_eq!(
            parse_config(args(&[]), Env::default()).unwrap(),
            run(default_listen(), None)
        );
        assert_eq!(
            parse_config(
                args(&[
                    "--listen",
                    "127.0.0.1:9000",
                    "--tls-cert",
                    "c.pem",
                    "--tls-key",
                    "k.pem"
                ]),
                Env::default(),
            )
            .unwrap(),
            run("127.0.0.1:9000".parse().unwrap(), Some(("c.pem", "k.pem")))
        );
        let env = Env {
            listen: Some("127.0.0.1:8000".into()),
            tls_cert: Some("env-c.pem".into()),
            tls_key: Some("env-k.pem".into()),
            ..Env::default()
        };
        assert_eq!(
            parse_config(args(&[]), env).unwrap(),
            run(
                "127.0.0.1:8000".parse().unwrap(),
                Some(("env-c.pem", "env-k.pem"))
            )
        );
        let env = Env {
            tls_cert: Some("env-c.pem".into()),
            tls_key: Some("env-k.pem".into()),
            ..Env::default()
        };
        assert_eq!(
            parse_config(args(&["--tls-cert", "cli-c.pem"]), env).unwrap(),
            run(default_listen(), Some(("cli-c.pem", "env-k.pem")))
        );
        let env = Env {
            tls_cert: Some(String::new()),
            tls_key: Some(String::new()),
            ..Env::default()
        };
        assert_eq!(
            parse_config(args(&[]), env).unwrap(),
            run(default_listen(), None)
        );
    }

    #[test]
    fn parses_websocket_listener_options() {
        let env = Env {
            ws_listen: Some("0.0.0.0:8080".into()),
            ws_origins: Some("https://a.example, https://b.example".into()),
            ws_client_ip_header: Some("CF-Connecting-IP".into()),
            ..Env::default()
        };
        assert_eq!(
            parse_config(args(&["--ws-origin", "https://c.example"]), env).unwrap(),
            Action::Run(Config {
                listen: default_listen(),
                tls: None,
                web: Some((
                    "0.0.0.0:8080".parse().unwrap(),
                    ws::Config {
                        origins: vec![
                            "https://a.example".into(),
                            "https://b.example".into(),
                            "https://c.example".into()
                        ],
                        client_ip_header: Some("cf-connecting-ip".into()),
                    }
                )),
            })
        );
        assert!(parse_config(args(&["--ws-origin", "https://a.example"]), Env::default()).is_err());
        assert!(parse_config(args(&["--ws-listen", "nope"]), Env::default()).is_err());
    }

    #[test]
    fn rejects_partial_tls_configuration_and_missing_values() {
        let only = |cert: Option<&str>, key: Option<&str>| Env {
            tls_cert: cert.map(str::to_owned),
            tls_key: key.map(str::to_owned),
            ..Env::default()
        };
        assert!(parse_config(args(&["--tls-cert", "c.pem"]), Env::default()).is_err());
        assert!(parse_config(args(&["--tls-key", "k.pem"]), Env::default()).is_err());
        assert!(parse_config(args(&[]), only(Some("c.pem"), None)).is_err());
        assert!(parse_config(args(&[]), only(None, Some("k.pem"))).is_err());
        assert!(parse_config(args(&["--tls-cert"]), Env::default()).is_err());
        assert!(parse_config(args(&["--tls-key"]), Env::default()).is_err());
        assert!(parse_config(args(&["--ws-listen"]), Env::default()).is_err());
    }
}
