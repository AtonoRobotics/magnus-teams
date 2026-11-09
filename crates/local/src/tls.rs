//! How this computer trusts the appliance: the certificate the person
//! pinned in the app, a paired device's certificate, or the public roots.
use anyhow::{anyhow, Result};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{ring, CryptoProvider};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, Error, SignatureScheme};
use std::sync::Arc;

#[derive(Clone, Debug, Default)]
pub enum Trust {
    #[default]
    PublicRoots,
    /// SHA-256 of the certificate as colon-separated hex, as the appliance prints it.
    Pinned(String),
    /// A paired device's certificate and key, both PEM.
    Device(String, String),
}

fn fingerprint(cert: &CertificateDer<'_>) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(cert.as_ref()).iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(":")
}

#[derive(Debug)]
struct Pinned {
    want: String,
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for Pinned {
    fn verify_server_cert(&self, end_entity: &CertificateDer<'_>, _: &[CertificateDer<'_>], _: &ServerName<'_>, _: &[u8], _: UnixTime) -> Result<ServerCertVerified, Error> {
        if fingerprint(end_entity) == self.want {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(Error::General("the appliance's certificate is not the one you trusted".into()))
        }
    }

    // The server must still prove it holds the certificate's key.
    fn verify_tls12_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn verify_tls13_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

/// The client to reach the appliance with.
pub fn apply(builder: reqwest::ClientBuilder, trust: &Trust) -> Result<reqwest::ClientBuilder> {
    Ok(match trust {
        Trust::PublicRoots => builder,
        Trust::Pinned(want) => {
            let provider = Arc::new(ring::default_provider());
            let verifier = Arc::new(Pinned { want: want.clone(), provider: provider.clone() });
            let tls = rustls::ClientConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .map_err(|e| anyhow!("{e}"))?
                .dangerous()
                .with_custom_certificate_verifier(verifier)
                .with_no_client_auth();
            builder.use_preconfigured_tls(tls)
        }
        Trust::Device(cert, key) => builder.identity(reqwest::Identity::from_pem(format!("{cert}\n{key}").as_bytes())?),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_pinned_certificate_is_accepted() {
        let cert = CertificateDer::from(vec![1, 2, 3]);
        let provider = Arc::new(ring::default_provider());
        let name = ServerName::try_from("appliance").unwrap();
        let check = |want: String| Pinned { want, provider: provider.clone() }.verify_server_cert(&cert, &[], &name, &[], UnixTime::now());
        assert!(check(fingerprint(&cert)).is_ok());
        assert!(check("00:11".to_string()).is_err());
    }
}
