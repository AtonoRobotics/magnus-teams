//! Each computer's own certificate authority, so the proxy can read what its
//! TLS carries. Its key is in the vault and stays on this side of the proxy.

use crate::vault::Vault;
use anyhow::{anyhow, Context, Result};
use rcgen::{BasicConstraints, Certificate, CertificateParams, DnType, IsCa, KeyPair};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Serialize, Deserialize)]
struct Stored {
    cert_pem: String,
    key_pem: String,
}

pub(in crate::computer) const SECRET_PREFIX: &str = "computer-ca-";

pub(in crate::computer) fn secret_name(computer: &str) -> String {
    format!("{SECRET_PREFIX}{computer}")
}

fn ca_params(computer: &str) -> Result<CertificateParams> {
    let mut params = CertificateParams::new(Vec::<String>::new())?;
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.distinguished_name.push(DnType::CommonName, format!("Magnus computer {computer}"));
    Ok(params)
}

fn stored(vault: &Vault, computer: &str) -> Result<Stored> {
    let name = secret_name(computer);
    if let Some(s) = vault.get(&name)? {
        return serde_json::from_str(s.expose()).context("a computer's CA is damaged");
    }
    let key = KeyPair::generate()?;
    let cert = ca_params(computer)?.self_signed(&key)?;
    let made = Stored { cert_pem: cert.pem(), key_pem: key.serialize_pem() };
    vault.put(&name, &serde_json::to_string(&made)?)?;
    Ok(made)
}

/// What the guest must trust.
pub fn certificate_pem(vault: &Vault, computer: &str) -> Result<String> {
    Ok(stored(vault, computer)?.cert_pem)
}

/// A TLS server identity for `host`, under this computer's CA.
pub fn server_config(vault: &Vault, computer: &str, host: &str) -> Result<Arc<rustls::ServerConfig>> {
    let ca = stored(vault, computer)?;
    let ca_key = KeyPair::from_pem(&ca.key_pem)?;
    // The same name and key as the certificate the guest holds, which is all
    // a leaf's signature depends on.
    let ca_cert: Certificate = ca_params(computer)?.self_signed(&ca_key)?;
    let leaf_key = KeyPair::generate()?;
    let leaf = CertificateParams::new(vec![host.to_string()])?.signed_by(&leaf_key, &ca_cert, &ca_key)?;
    let key = rustls::pki_types::PrivateKeyDer::try_from(leaf_key.serialize_der()).map_err(|e| anyhow!("{e}"))?;
    let mut config = rustls::ServerConfig::builder_with_provider(rustls::crypto::ring::default_provider().into())
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_single_cert(vec![leaf.der().clone()], key)?;
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(Arc::new(config))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn a_guest_that_trusts_the_ca_accepts_the_proxys_certificate() {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::open(dir.path()).unwrap();
        let pem = certificate_pem(&vault, "a").unwrap();
        assert_eq!(pem, certificate_pem(&vault, "a").unwrap(), "the CA changed between uses");
        assert_ne!(pem, certificate_pem(&vault, "b").unwrap(), "two computers share a CA");

        let server = tokio_rustls::TlsAcceptor::from(server_config(&vault, "a", "site.test").unwrap());
        let mut roots = rustls::RootCertStore::empty();
        use rustls::pki_types::pem::PemObject;
        roots.add(rustls::pki_types::CertificateDer::from_pem_slice(pem.as_bytes()).unwrap()).unwrap();
        let client_config = rustls::ClientConfig::builder_with_provider(rustls::crypto::ring::default_provider().into())
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let client = tokio_rustls::TlsConnector::from(Arc::new(client_config));
        let (a, b) = tokio::io::duplex(1 << 16);
        let served = tokio::spawn(async move {
            let mut s = server.accept(a).await.unwrap();
            s.write_all(b"ok").await.unwrap();
            s.shutdown().await.unwrap();
        });
        let name = rustls::pki_types::ServerName::try_from("site.test").unwrap();
        let mut stream = client.connect(name, b).await.expect("the guest refused the certificate");
        let mut got = vec![];
        stream.read_to_end(&mut got).await.unwrap();
        served.await.unwrap();
        assert_eq!(got, b"ok");
    }
}
