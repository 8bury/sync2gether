use quinn::{ClientConfig, ServerConfig, TransportConfig, VarInt};
use rustls::{
    DigitallySignedStruct, DistinguishedName, Error as TlsError, SignatureScheme,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature},
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime},
    server::danger::{ClientCertVerified, ClientCertVerifier},
};
use std::{io, sync::Arc, time::Duration};

pub const ALPN: &[u8] = b"sync2gether-stream/1";

#[derive(Clone)]
pub struct Identity {
    pub cert: CertificateDer<'static>,
    pub key: Vec<u8>,
    pub fingerprint: String,
}

impl Identity {
    pub fn generate() -> io::Result<Self> {
        let certified = rcgen::generate_simple_self_signed(vec!["sync2gether.peer".into()])
            .map_err(io::Error::other)?;
        let cert = certified.cert.der().clone();
        let key = certified.signing_key.serialize_der();
        let fingerprint = blake3::hash(cert.as_ref()).to_hex().to_string();
        Ok(Self {
            cert,
            key,
            fingerprint,
        })
    }

    fn private_key(&self) -> PrivateKeyDer<'static> {
        PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(self.key.clone()))
    }
}

pub fn fingerprint_of(cert: &CertificateDer<'_>) -> String {
    blake3::hash(cert.as_ref()).to_hex().to_string()
}

pub fn valid_fingerprint(fp: &str) -> bool {
    fp.len() == 64 && fp.bytes().all(|b| b.is_ascii_hexdigit())
}

fn transport_config() -> Arc<TransportConfig> {
    let mut config = TransportConfig::default();
    config.max_concurrent_bidi_streams(VarInt::from_u32(64));
    config.max_concurrent_uni_streams(VarInt::from_u32(16));
    config.keep_alive_interval(Some(Duration::from_secs(5)));
    if let Ok(idle) = Duration::from_secs(30).try_into() {
        config.max_idle_timeout(Some(idle));
    }
    config.stream_receive_window(VarInt::from_u32(8 * 1024 * 1024));
    config.receive_window(VarInt::from_u32(32 * 1024 * 1024));
    config.send_window(32 * 1024 * 1024);
    Arc::new(config)
}

#[derive(Debug)]
struct PinnedServerVerifier {
    expected_fingerprint: String,
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for PinnedServerVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, TlsError> {
        if fingerprint_of(end_entity).eq_ignore_ascii_case(&self.expected_fingerprint) {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(TlsError::InvalidCertificate(
                rustls::CertificateError::ApplicationVerificationFailure,
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

#[derive(Debug)]
struct PinnedClientVerifier {
    expected_fingerprint: String,
    provider: Arc<CryptoProvider>,
}

impl ClientCertVerifier for PinnedClientVerifier {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }

    fn verify_client_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> Result<ClientCertVerified, TlsError> {
        if fingerprint_of(end_entity).eq_ignore_ascii_case(&self.expected_fingerprint) {
            Ok(ClientCertVerified::assertion())
        } else {
            Err(TlsError::InvalidCertificate(
                rustls::CertificateError::ApplicationVerificationFailure,
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

pub fn server_config(identity: &Identity, expected_client_fp: &str) -> io::Result<ServerConfig> {
    if !valid_fingerprint(expected_client_fp) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Impressão digital do cliente inválida",
        ));
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let verifier = Arc::new(PinnedClientVerifier {
        expected_fingerprint: expected_client_fp.to_owned(),
        provider: provider.clone(),
    });
    let mut tls = rustls::ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(io::Error::other)?
        .with_client_cert_verifier(verifier)
        .with_single_cert(vec![identity.cert.clone()], identity.private_key())
        .map_err(io::Error::other)?;
    tls.alpn_protocols = vec![ALPN.to_vec()];
    let quic = quinn::crypto::rustls::QuicServerConfig::try_from(tls).map_err(io::Error::other)?;
    let mut config = ServerConfig::with_crypto(Arc::new(quic));
    config.transport_config(transport_config());
    Ok(config)
}

pub fn client_config(identity: &Identity, expected_server_fp: &str) -> io::Result<ClientConfig> {
    if !valid_fingerprint(expected_server_fp) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Impressão digital do servidor inválida",
        ));
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let verifier = Arc::new(PinnedServerVerifier {
        expected_fingerprint: expected_server_fp.to_owned(),
        provider: provider.clone(),
    });
    let mut tls = rustls::ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(io::Error::other)?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_client_auth_cert(vec![identity.cert.clone()], identity.private_key())
        .map_err(io::Error::other)?;
    tls.alpn_protocols = vec![ALPN.to_vec()];
    let quic = quinn::crypto::rustls::QuicClientConfig::try_from(tls).map_err(io::Error::other)?;
    let mut config = ClientConfig::new(Arc::new(quic));
    config.transport_config(transport_config());
    Ok(config)
}
