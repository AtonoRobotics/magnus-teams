//! HTTPS to the appliance with its certificate pinned on first use, as SSH
//! pins a host key. The appliance usually has a self-signed certificate and
//! no public name, so there is no authority to ask; the person compares the
//! fingerprint once with the one the owner sees on the appliance.

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{ring, CryptoProvider};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, Error, SignatureScheme};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// SHA-256 of the certificate as colon-separated hex pairs, as the appliance prints it.
pub fn fingerprint(cert: &CertificateDer<'_>) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(cert.as_ref()).iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(":")
}

#[derive(Debug)]
struct Pinned {
    /// None only for the first look, which sends no credentials.
    want: Option<String>,
    seen: Arc<Mutex<Option<String>>>,
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for Pinned {
    fn verify_server_cert(&self, end_entity: &CertificateDer<'_>, _: &[CertificateDer<'_>], _: &ServerName<'_>, _: &[u8], _: UnixTime) -> Result<ServerCertVerified, Error> {
        let fp = fingerprint(end_entity);
        *self.seen.lock().unwrap() = Some(fp.clone());
        match &self.want {
            Some(w) if *w != fp => Err(Error::General("the appliance's certificate is not the one you trusted".into())),
            _ => Ok(ServerCertVerified::assertion()),
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

fn config(want: Option<String>, seen: Arc<Mutex<Option<String>>>) -> Result<rustls::ClientConfig, String> {
    let provider = Arc::new(ring::default_provider());
    let verifier = Arc::new(Pinned { want, seen, provider: provider.clone() });
    Ok(rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth())
}

/// TLS that trusts only the pinned certificate, for the updater's own client.
pub fn tls(pin: Option<&str>) -> Result<rustls::ClientConfig, String> {
    config(pin.map(String::from), Arc::new(Mutex::new(None)))
}

fn build(want: Option<String>) -> Result<(reqwest::Client, Arc<Mutex<Option<String>>>), String> {
    let seen = Arc::new(Mutex::new(None));
    let tls = config(want, seen.clone())?;
    // Versions both ways: the appliance answers with its own and refuses an app too old for it.
    let mut headers = reqwest::header::HeaderMap::new();
    if let Ok(v) = reqwest::header::HeaderValue::from_str(&crate::app_version()) {
        headers.insert("x-magnus-app", v);
    }
    let client = reqwest::Client::builder()
        .default_headers(headers)
        .use_preconfigured_tls(tls)
        .connect_timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;
    Ok((client, seen))
}

/// A client that talks only to the appliance holding the pinned certificate
/// (or plain HTTP, which the appliance accepts for sign-in only from itself).
pub fn client(pin: Option<&str>) -> Result<reqwest::Client, String> {
    build(pin.map(String::from)).map(|c| c.0)
}

/// Connect once without trusting anything, to learn the certificate's
/// fingerprint. Nothing secret is sent on this connection.
pub async fn look(url: &str) -> Result<String, String> {
    let (c, seen) = build(None)?;
    let r = c.get(format!("{url}/v1/health")).send().await;
    let fp = seen.lock().unwrap().clone();
    match (fp, r) {
        (Some(fp), _) => Ok(fp),
        (None, Err(e)) => Err(describe(&e)),
        (None, Ok(_)) => Err("the appliance did not present a certificate".into()),
    }
}

/// The public appliance. Its certificate is Cloudflare's, which the public
/// roots already trust, so it is not pinned. Every other address keeps the
/// pin: a LAN address, a `.local` name, an IP, Tailscale.
pub fn is_public_appliance(url: &str) -> bool {
    let Ok(u) = url::Url::parse(url) else { return false };
    u.scheme() == "https" && u.host_str().is_some_and(|h| h.eq_ignore_ascii_case("magnus.pyrallon.ai"))
}

/// A key and a certificate request, made here. The key is not in the request.
pub fn begin_device() -> Result<(String, String), String> {
    let key = rcgen::KeyPair::generate().map_err(|e| e.to_string())?;
    let mut params = rcgen::CertificateParams::new(Vec::<String>::new()).map_err(|e| e.to_string())?;
    params.distinguished_name.push(rcgen::DnType::CommonName, "magnus-device");
    let csr = params.serialize_request(&key).map_err(|e| e.to_string())?;
    Ok((key.serialize_pem(), csr.pem().map_err(|e| e.to_string())?))
}

fn identity(cert_pem: &str, key_pem: &str) -> Result<(Vec<CertificateDer<'static>>, rustls::pki_types::PrivateKeyDer<'static>), String> {
    let mut certs = std::io::Cursor::new(cert_pem.as_bytes());
    let certs = rustls_pemfile::certs(&mut certs).collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    if certs.is_empty() {
        return Err("the device certificate is empty".into());
    }
    let mut key = std::io::Cursor::new(key_pem.as_bytes());
    let key = rustls_pemfile::private_key(&mut key).map_err(|e| e.to_string())?.ok_or("the device key is empty")?;
    Ok((certs, key))
}

/// TLS that trusts the public roots and presents this computer's device
/// certificate. Used only for `magnus.pyrallon.ai`.
/// TLS for the public appliance: the public roots and this computer's device certificate.
pub fn public_tls(cert_pem: &str, key_pem: &str) -> Result<rustls::ClientConfig, String> {
    let provider = Arc::new(ring::default_provider());
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let (certs, key) = identity(cert_pem, key_pem)?;
    rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .with_root_certificates(roots)
        .with_client_auth_cert(certs, key)
        .map_err(|e| e.to_string())
}

pub fn public_client(cert_pem: &str, key_pem: &str) -> Result<reqwest::Client, String> {
    let mut tls = public_tls(cert_pem, key_pem)?;
    tls.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    let mut headers = reqwest::header::HeaderMap::new();
    if let Ok(v) = reqwest::header::HeaderValue::from_str(&crate::app_version()) {
        headers.insert("x-magnus-app", v);
    }
    reqwest::Client::builder().default_headers(headers).use_preconfigured_tls(tls).connect_timeout(Duration::from_secs(10)).build().map_err(|e| e.to_string())
}

/// The innermost cause of a request error, which is the one a person can act on.
pub fn describe(e: &reqwest::Error) -> String {
    let mut msg = e.to_string();
    let mut src: Option<&dyn std::error::Error> = std::error::Error::source(e);
    while let Some(s) = src {
        msg = s.to_string();
        src = s.source();
    }
    msg
}
