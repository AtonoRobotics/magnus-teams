//! The admin console set up on an appliance by `release/admin-on-appliance.sh`
//! (DECISIONS.md, "The admin console on strix-alpha"): from an appliance's
//! config with no `[admin]`, it writes the machine's Tailscale name as the
//! console's hostname and its addresses as aliases, listens only on the
//! machine's address, takes only that network, and makes the organisation's
//! CA and the console's certificate from it, naming every name. The console
//! starts on what it wrote and answers each of its names over TLS that the
//! new CA verifies, and nothing else; run again, it writes nothing more. A
//! section an earlier run wrote, with one name, is brought up to every name
//! and keeps its database password. On a tailnet, the console's name
//! there gets the certificate the tailnet issues, and its addresses keep
//! the CA's.

use magnus_invariants::appliance::{bin, workspace_root};
use magnus_invariants::*;
use magnus_scratch::admin::{free_port, lan_ip, presented_at, FakeTailscale};
use std::net::SocketAddr;

fn run(config: &std::path::Path, lan: &str, port: u16) {
    let out = std::process::Command::new(workspace_root().join("release/admin-on-appliance.sh"))
        .arg(config)
        .arg(bin("magnus-admin").parent().unwrap())
        .arg("--config-only")
        .env("MAGNUS_ADMIN_LAN", lan)
        .env("MAGNUS_ADMIN_TAILSCALE", "none")
        .env("MAGNUS_ADMIN_TSNAME", "strix.example-tailnet.ts.net")
        .env("MAGNUS_ADMIN_PORT", port.to_string())
        .output()
        .unwrap();
    assert!(out.status.success(), "the setup failed:\n{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_setup_writes_an_admin_console_that_answers_its_hostname_on_the_network_only() {
    let m = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(Spec::default(), &m).await;
    // An appliance signing people in through its identity provider, as
    // strix-alpha does; the console never asks it before anyone signs in.
    let config = app.dir.path().join("magnus.toml");
    let mut cfg: toml::Table = std::fs::read_to_string(&config).unwrap().parse().unwrap();
    let mut oidc = toml::Table::new();
    oidc.insert("issuer".into(), "http://127.0.0.1:9/realms/unused".into());
    oidc.insert("client_id".into(), "magnus-claude".into());
    let mut sign_in = toml::Table::new();
    sign_in.insert("oidc".into(), oidc.into());
    cfg.insert("sign_in".into(), sign_in.into());
    std::fs::write(&config, toml::to_string(&cfg).unwrap()).unwrap();

    let ip = lan_ip();
    let port = free_port(ip);
    // What #78's first run wrote on strix-alpha: one name, no aliases.
    let before = std::fs::read_to_string(&config).unwrap();
    std::fs::write(&config, format!("{before}\n# The admin console (written by release/admin-on-appliance.sh; admin console spec).\n[admin]\nhostname = \"strix-alpha.local\"\nlisten = [\"{ip}:{port}\"]\nallow = [\"{ip}/32\"]\ntls_cert = \"x\"\ntls_key = \"y\"\ndatabase_url = \"postgres://magnus_root:kept@127.0.0.1:1/magnus\"\n")).unwrap();
    run(&config, &format!("{ip}/24"), port);
    let written = std::fs::read_to_string(&config).unwrap();
    let cfg: toml::Table = written.parse().unwrap();
    let a = cfg["admin"].as_table().unwrap();
    let host = a["hostname"].as_str().unwrap().to_string();
    assert_eq!(host, "strix.example-tailnet.ts.net");
    assert_eq!(a["aliases"].as_array().unwrap(), &vec![toml::Value::from(ip.to_string())], "the address is not an alias");
    assert_eq!(a["listen"].as_array().unwrap(), &vec![toml::Value::from(format!("{ip}:{port}"))]);
    let net = a["allow"].as_array().unwrap()[0].as_str().unwrap().to_string();
    assert!(net.ends_with("/24") && !net.starts_with("0.0.0.0"), "{net}");
    assert_eq!(a["database_url"].as_str().unwrap(), "postgres://magnus_root:kept@127.0.0.1:1/magnus", "the database password was not kept");
    let tls = app.dir.path().join("admin-tls");
    let mode = |f: &str| std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(tls.join(f)).unwrap().permissions()) & 0o777;
    assert_eq!((mode("ca.key"), mode("console.key")), (0o600, 0o600), "a key is readable by others");

    // On a tailnet, its name there gets a certificate browsers trust, kept beside the CA's.
    assert_eq!(a["trusted_via"].as_str(), Some("tailscale"));
    assert_eq!((a["trusted_cert"].as_str(), a["trusted_key"].as_str()), (Some(tls.join("trusted.pem").to_str().unwrap()), Some(tls.join("trusted.key").to_str().unwrap())));

    // Run again: nothing more is written.
    run(&config, &format!("{ip}/24"), port);
    assert_eq!(std::fs::read_to_string(&config).unwrap().matches("[admin]").count(), 1, "the setup wrote [admin] twice");

    // The tailnet, a stand-in (never this machine's own tailscaled): not
    // issuing yet, so every name gets the CA's certificate.
    let ts = FakeTailscale::new();
    ts.fail("500 Internal Server Error: your Tailscale account does not support getting TLS certs");
    let written = std::fs::read_to_string(&config).unwrap();
    assert!(written.trim_end().ends_with(&format!("database_url = \"{}\"", a["database_url"].as_str().unwrap())), "[admin] is not the config's last table");
    std::fs::write(&config, format!("{}\ntailscale = \"{}\"\n", written.trim_end(), ts.program.display())).unwrap();

    // The console starts on it, and answers its hostname over TLS the CA verifies.
    let mut child = tokio::process::Command::new(bin("magnus-admin")).arg("--config").arg(&config).kill_on_drop(true).spawn().unwrap();
    let addr = SocketAddr::new(ip, port);
    eventually("the console listening", PATIENCE, || async { tokio::net::TcpStream::connect(addr).await.ok().map(|_| ()) }).await;
    let ca = reqwest::Certificate::from_pem(&std::fs::read(tls.join("ca.pem")).unwrap()).unwrap();
    let browser = reqwest::Client::builder().add_root_certificate(ca).resolve(&host, addr).no_proxy().redirect(reqwest::redirect::Policy::none()).build().unwrap();
    let r = browser.get(format!("https://{host}:{port}/")).send().await.expect("the console's certificate is not trusted through the new CA");
    assert!(r.status().is_success() || r.status().is_redirection(), "{}", r.status());
    // By its address too, as a computer on the LAN types it.
    let ca = reqwest::Certificate::from_pem(&std::fs::read(tls.join("ca.pem")).unwrap()).unwrap();
    let by_ip = reqwest::Client::builder().add_root_certificate(ca).no_proxy().redirect(reqwest::redirect::Policy::none()).build().unwrap();
    let r = by_ip.get(format!("https://{ip}:{port}/")).send().await.expect("the certificate does not name the address");
    assert!(r.status().is_success() || r.status().is_redirection(), "the console did not answer its address: {}", r.status());
    let other = browser.get(format!("https://{host}:{port}/")).header("Host", "elsewhere.example").send().await.unwrap();
    assert_eq!(other.status().as_u16(), 404, "the console answered another hostname");
    let _ = child.start_kill();
    let _ = child.wait().await;

    // Issued: its tailnet name gets the trusted certificate; its address
    // still the CA's.
    let issued = ts.ready(&host);
    let mut child = tokio::process::Command::new(bin("magnus-admin")).arg("--config").arg(&config).kill_on_drop(true).spawn().unwrap();
    let seen = eventually("the trusted certificate served", PATIENCE, || async { presented_at(addr, &host, ts.public_ca()).await.ok() }).await;
    assert_eq!(seen, issued);
    assert!(std::fs::metadata(tls.join("trusted.key")).is_ok(), "the issued key is not kept beside the CA's");
    let ca = reqwest::Certificate::from_pem(&std::fs::read(tls.join("ca.pem")).unwrap()).unwrap();
    let by_ip = reqwest::Client::builder().add_root_certificate(ca).no_proxy().redirect(reqwest::redirect::Policy::none()).build().unwrap();
    by_ip.get(format!("https://{ip}:{port}/")).send().await.expect("the address no longer gets the CA's certificate");
    let _ = child.start_kill();
}

/// Past `--config-only`, the setup names Keycloak's admin origin, admin
/// file and CA explicitly from the appliance's config: with no appliance
/// identity configured it stops and says which setting is missing (#312),
/// instead of reaching the retired deployment's default CA and dying in
/// `ssl.create_default_context`.
#[tokio::test(flavor = "multi_thread")]
async fn the_setup_names_keycloaks_admin_origin_from_the_config_or_says_what_is_missing() {
    let m = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(Spec::default(), &m).await;
    let config = app.dir.path().join("magnus.toml");
    let ip = lan_ip();
    let port = free_port(ip);
    run(&config, &format!("{ip}/24"), port);
    let out = std::process::Command::new(workspace_root().join("release/admin-on-appliance.sh"))
        .arg(&config)
        .arg(bin("magnus-admin").parent().unwrap())
        .env("MAGNUS_ADMIN_LAN", format!("{ip}/24"))
        .env("MAGNUS_ADMIN_TAILSCALE", "none")
        .env("MAGNUS_ADMIN_TSNAME", "strix.example-tailnet.ts.net")
        .env("MAGNUS_ADMIN_PORT", port.to_string())
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "the setup reached Keycloak with no identity configured");
    assert!(stderr.contains("status_admin_ca"), "it did not say which setting is missing: {stderr}");
}

/// `config/keycloak-client.py` takes its admin origin, admin file and CA
/// as required arguments (#428): the retired deployment's defaults are
/// gone, so a bare run fails naming them instead of dying on a CA file
/// that exists nowhere.
#[test]
fn the_client_script_requires_its_admin_origin_file_and_ca() {
    let out = std::process::Command::new("python3").arg("-I").arg(workspace_root().join("config/keycloak-client.py")).output().unwrap();
    assert_eq!(out.status.code(), Some(2), "a bare run did not fail as argparse");
    let stderr = String::from_utf8_lossy(&out.stderr);
    for a in ["--admin-url", "--admin-file", "--ca"] {
        assert!(stderr.contains(a), "it did not name {a}: {stderr}");
    }
}
