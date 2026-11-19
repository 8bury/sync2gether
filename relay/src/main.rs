use std::{net::SocketAddr, path::PathBuf};

#[derive(Debug, PartialEq, Eq)]
struct Config {
    listen: SocketAddr,
    tls: Option<(PathBuf, PathBuf)>,
}

#[derive(Debug, PartialEq, Eq)]
enum Action {
    Run(Config),
    Print(String),
}

fn parse_config(
    args: impl IntoIterator<Item = String>,
    env_listen: Option<String>,
    env_tls_cert: Option<String>,
    env_tls_key: Option<String>,
) -> Result<Action, String> {
    let mut args = args.into_iter();
    let mut listen = env_listen.filter(|value| !value.is_empty());
    let mut tls_cert = env_tls_cert
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let mut tls_key = env_tls_key
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--listen" => listen = Some(args.next().ok_or("--listen requer um endereço")?),
            "--tls-cert" => {
                tls_cert = Some(PathBuf::from(
                    args.next().ok_or("--tls-cert requer um caminho")?,
                ))
            }
            "--tls-key" => {
                tls_key = Some(PathBuf::from(
                    args.next().ok_or("--tls-key requer um caminho")?,
                ))
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
                     Padrão: 0.0.0.0:{}. Também aceita RELAY_LISTEN, RELAY_TLS_CERT e RELAY_TLS_KEY.",
                    sync2gether_relay::PORT
                )));
            }
            other => return Err(format!("Argumento desconhecido: {other}")),
        }
    }
    let listen = listen
        .unwrap_or_else(|| format!("0.0.0.0:{}", sync2gether_relay::PORT))
        .parse()
        .map_err(|_| "Endereço inválido; use IP:PORTA".to_owned())?;
    let tls = sync2gether_relay::tls::resolve_paths(tls_cert, tls_key)?;
    Ok(Action::Run(Config { listen, tls }))
}

#[tokio::main]
async fn main() {
    let config = match parse_config(
        std::env::args().skip(1),
        std::env::var("RELAY_LISTEN").ok(),
        std::env::var("RELAY_TLS_CERT").ok(),
        std::env::var("RELAY_TLS_KEY").ok(),
    ) {
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
    let listener = match tokio::net::TcpListener::bind(config.listen).await {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("Não foi possível escutar em {}: {error}", config.listen);
            std::process::exit(1);
        }
    };
    let mode = if acceptor.is_some() { "TLS" } else { "TCP" };
    eprintln!(
        "sync2gether-relay {} escutando em {} ({mode}), protocolo {}",
        env!("CARGO_PKG_VERSION"),
        config.listen,
        sync2gether_relay::VERSION
    );
    let report = |stats: sync2gether_relay::Stats| {
        eprintln!(
            "conexões={} salas={} pareadas={}",
            stats.connections, stats.rooms, stats.pipes
        )
    };
    let mut terminate =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).ok();
    let serve = async {
        match acceptor {
            Some(acceptor) => {
                sync2gether_relay::serve_tls_with_stats(
                    listener,
                    sync2gether_relay::Limits::default(),
                    acceptor,
                    report,
                )
                .await
            }
            None => {
                sync2gether_relay::serve_with_stats(
                    listener,
                    sync2gether_relay::Limits::default(),
                    report,
                )
                .await
            }
        }
    };
    tokio::select! {
        _ = serve => {}
        _ = tokio::signal::ctrl_c() => {}
        _ = async { match &mut terminate { Some(signal) => { signal.recv().await; } None => std::future::pending().await } } => {}
    }
    eprintln!("Encerrando o relay");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn parses_plain_and_tls_options_from_flags_and_env() {
        assert_eq!(
            parse_config(args(&[]), None, None, None).unwrap(),
            Action::Run(Config {
                listen: format!("0.0.0.0:{}", sync2gether_relay::PORT)
                    .parse()
                    .unwrap(),
                tls: None,
            })
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
                None,
                None,
                None,
            )
            .unwrap(),
            Action::Run(Config {
                listen: "127.0.0.1:9000".parse().unwrap(),
                tls: Some((PathBuf::from("c.pem"), PathBuf::from("k.pem"))),
            })
        );
        assert_eq!(
            parse_config(
                args(&[]),
                Some("127.0.0.1:8000".into()),
                Some("env-c.pem".into()),
                Some("env-k.pem".into()),
            )
            .unwrap(),
            Action::Run(Config {
                listen: "127.0.0.1:8000".parse().unwrap(),
                tls: Some((PathBuf::from("env-c.pem"), PathBuf::from("env-k.pem"))),
            })
        );
        assert_eq!(
            parse_config(
                args(&["--tls-cert", "cli-c.pem"]),
                None,
                Some("env-c.pem".into()),
                Some("env-k.pem".into()),
            )
            .unwrap(),
            Action::Run(Config {
                listen: format!("0.0.0.0:{}", sync2gether_relay::PORT)
                    .parse()
                    .unwrap(),
                tls: Some((PathBuf::from("cli-c.pem"), PathBuf::from("env-k.pem"))),
            })
        );
        assert_eq!(
            parse_config(args(&[]), None, Some(String::new()), Some(String::new())).unwrap(),
            Action::Run(Config {
                listen: format!("0.0.0.0:{}", sync2gether_relay::PORT)
                    .parse()
                    .unwrap(),
                tls: None,
            })
        );
    }

    #[test]
    fn rejects_partial_tls_configuration_and_missing_values() {
        assert!(parse_config(args(&["--tls-cert", "c.pem"]), None, None, None).is_err());
        assert!(parse_config(args(&["--tls-key", "k.pem"]), None, None, None).is_err());
        assert!(parse_config(args(&[]), None, Some("c.pem".into()), None).is_err());
        assert!(parse_config(args(&[]), None, None, Some("k.pem".into())).is_err());
        assert!(parse_config(args(&["--tls-cert"]), None, None, None).is_err());
        assert!(parse_config(args(&["--tls-key"]), None, None, None).is_err());
    }
}
