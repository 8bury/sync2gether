use rustls::{ServerConfig, pki_types::CertificateDer};
use std::{
    io::{self, Cursor},
    path::{Path, PathBuf},
    sync::Arc,
};
pub use tokio_rustls::TlsAcceptor;

pub fn resolve_paths(
    cert: Option<PathBuf>,
    key: Option<PathBuf>,
) -> Result<Option<(PathBuf, PathBuf)>, String> {
    match (cert, key) {
        (None, None) => Ok(None),
        (Some(cert), Some(key)) => Ok(Some((cert, key))),
        (Some(_), None) | (None, Some(_)) => {
            Err("Informe --tls-cert e --tls-key juntos (ou RELAY_TLS_CERT e RELAY_TLS_KEY).".into())
        }
    }
}

pub fn load_acceptor(cert: &Path, key: &Path) -> io::Result<TlsAcceptor> {
    let cert_pem = std::fs::read(cert).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("Não foi possível ler o certificado TLS: {error}"),
        )
    })?;
    let key_pem = std::fs::read(key).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("Não foi possível ler a chave privada TLS: {error}"),
        )
    })?;
    acceptor_from_pem(&cert_pem, &key_pem)
}

pub fn acceptor_from_pem(cert_pem: &[u8], key_pem: &[u8]) -> io::Result<TlsAcceptor> {
    let mut cert_reader = Cursor::new(cert_pem);
    let certs: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut cert_reader)
        .collect::<Result<_, _>>()
        .map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Certificado TLS inválido: {error}"),
            )
        })?;
    if certs.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Nenhum certificado encontrado no arquivo PEM",
        ));
    }
    let mut key_reader = Cursor::new(key_pem);
    let key = rustls_pemfile::private_key(&mut key_reader)
        .map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Chave privada TLS inválida: {error}"),
            )
        })?
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "Nenhuma chave privada encontrada no arquivo PEM",
            )
        })?;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Configuração TLS inválida: {error}"),
            )
        })?
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Par certificado/chave TLS inválido: {error}"),
            )
        })?;
    Ok(TlsAcceptor::from(Arc::new(config)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_paths_requires_both_cert_and_key_or_neither() {
        assert_eq!(resolve_paths(None, None).unwrap(), None);
        assert_eq!(
            resolve_paths(Some(PathBuf::from("a.pem")), Some(PathBuf::from("b.pem"))).unwrap(),
            Some((PathBuf::from("a.pem"), PathBuf::from("b.pem")))
        );
        assert!(resolve_paths(Some(PathBuf::from("a.pem")), None).is_err());
        assert!(resolve_paths(None, Some(PathBuf::from("b.pem"))).is_err());
    }

    #[test]
    fn acceptor_validates_pem_inputs_and_mismatched_pairs() {
        let first = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let second = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let cert = first.cert.pem();
        let key = first.signing_key.serialize_pem();
        let other_key = second.signing_key.serialize_pem();

        assert!(acceptor_from_pem(cert.as_bytes(), key.as_bytes()).is_ok());
        assert!(acceptor_from_pem(b"", key.as_bytes()).is_err());
        assert!(acceptor_from_pem(cert.as_bytes(), b"").is_err());
        assert!(acceptor_from_pem(b"not a pem", key.as_bytes()).is_err());
        assert!(acceptor_from_pem(cert.as_bytes(), other_key.as_bytes()).is_err());
        assert!(
            load_acceptor(
                Path::new("/nao/existe/cert.pem"),
                Path::new("/nao/existe/key.pem")
            )
            .is_err()
        );
    }
}
