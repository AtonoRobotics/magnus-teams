//! A real Keycloak for sign-in tests: a fresh realm per test on the dev and
//! test Keycloak (`dev/keycloak.sh`; `MAGNUS_TEST_KEYCLOAK`), deleted when the
//! test ends, as each test gets its own database on the test Postgres.
//!
//! The realm is set up the way docs/proposals/accounts.md asks the owner to
//! set up theirs: a public client with Direct access grants (the password
//! typed in the app), the authorization code flow with PKCE (S256) and
//! loopback redirects, the device grant for browsers, and the person's
//! groups and realm roles in the ID token; and a status client whose service
//! account may only view users. People sign in on Keycloak's own pages in a
//! real Chromium (`tests/scratch/sign-in-browser.mjs`).

use magnus_invariants::appliance::{workspace_root, Appliance};
use serde_json::{json, Value};
use std::collections::HashMap;

/// The client the appliance signs in as, and what its button says.
pub const IDP_CLIENT: &str = "magnus-test";
pub const IDP_LABEL: &str = "Test identity";
/// The status client, and the vault secret the appliance keeps its secret in.
pub const STATUS_CLIENT: &str = "magnus-test-status";
pub const STATUS_CREDENTIAL: &str = "keycloak_status";

pub fn server_url() -> String {
    std::env::var("MAGNUS_TEST_KEYCLOAK").unwrap_or_else(|_| "http://127.0.0.1:54380".into())
}

/// The server's admin, as `dev/keycloak.sh` makes it.
fn admin_login() -> (String, String) {
    (std::env::var("MAGNUS_TEST_KEYCLOAK_ADMIN").unwrap_or_else(|_| "admin".into()), std::env::var("MAGNUS_TEST_KEYCLOAK_PASSWORD").unwrap_or_else(|_| "magnus_dev".into()))
}

/// A person's password in the test realm.
pub fn password_of(username: &str) -> String {
    format!("{}-Pw-7f3a", username.to_lowercase())
}

pub struct Keycloak {
    pub realm: String,
    pub issuer: String,
    status_secret: String,
    http: reqwest::Client,
}

/// The code an authenticator app shows now for this secret (RFC 6238: SHA-1,
/// 6 digits, 30 seconds, as Keycloak's default OTP policy).
pub fn code_for(secret: &str) -> String {
    use hmac::Mac;
    let step = (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() / 30).to_be_bytes();
    let mut m = hmac::Hmac::<sha1::Sha1>::new_from_slice(secret.as_bytes()).unwrap();
    m.update(&step);
    let h = m.finalize().into_bytes();
    let o = (h[19] & 15) as usize;
    let n = u32::from_be_bytes([h[o] & 0x7f, h[o + 1], h[o + 2], h[o + 3]]) % 1_000_000;
    format!("{n:06}")
}

async fn admin_token(http: &reqwest::Client) -> String {
    let (user, pw) = admin_login();
    let r = http
        .post(format!("{}/realms/master/protocol/openid-connect/token", server_url()))
        .form(&[("grant_type", "password"), ("client_id", "admin-cli"), ("username", &user), ("password", &pw)])
        .send()
        .await
        .unwrap_or_else(|e| panic!("the test Keycloak at {} does not answer (start it with dev/keycloak.sh): {e}", server_url()));
    assert!(r.status().is_success(), "the test Keycloak refused its admin: {}", r.status());
    r.json::<Value>().await.unwrap()["access_token"].as_str().unwrap().to_string()
}

/// How long a test waits for any one HTTP answer from Keycloak, the console
/// or the appliance: a request that never answers fails its test, named,
/// instead of holding it until the runner kills it at 600 s with nothing to
/// say (#239). Two minutes is above every answer these servers give: the
/// console's own calls to Keycloak time out at 30 s, and its slowest request
/// (an invitation) makes three of them.
pub const HTTP_PATIENCE: std::time::Duration = std::time::Duration::from_secs(120);

/// An HTTP client for the tests that gives up after `HTTP_PATIENCE`.
pub fn http() -> reqwest::Client {
    reqwest::Client::builder().timeout(HTTP_PATIENCE).build().unwrap()
}

impl Keycloak {
    /// A new realm on the test Keycloak, with the client and the mappers.
    pub async fn start() -> Keycloak {
        let http = http();
        let realm = format!("t{}", uuid::Uuid::new_v4().simple());
        let status_secret = uuid::Uuid::new_v4().simple().to_string();
        let mapper = |name: &str, kind: &str, config: Value| json!({ "name": name, "protocol": "openid-connect", "protocolMapper": kind, "config": config });
        let claims = json!({ "id.token.claim": "true", "access.token.claim": "true", "userinfo.token.claim": "true" });
        let with = |extra: Value| {
            let mut c = claims.clone();
            c.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            c
        };
        let body = json!({
            "realm": realm,
            "enabled": true,
            "clients": [{
                "clientId": IDP_CLIENT,
                "name": "Magnus",
                "publicClient": true,
                "standardFlowEnabled": true,
                "directAccessGrantsEnabled": true,
                "redirectUris": ["http://127.0.0.1/*"],
                "attributes": { "pkce.code.challenge.method": "S256", "oauth2.device.authorization.grant.enabled": "true" },
                "protocolMappers": [
                    mapper("groups", "oidc-group-membership-mapper", with(json!({ "claim.name": "groups", "full.path": "true" }))),
                    mapper("realm roles", "oidc-usermodel-realm-role-mapper", with(json!({ "claim.name": "realm_access.roles", "multivalued": "true" }))),
                ],
            }, {
                "clientId": STATUS_CLIENT,
                "publicClient": false,
                "secret": status_secret,
                "serviceAccountsEnabled": true,
                "standardFlowEnabled": false,
                "directAccessGrantsEnabled": false,
            }],
        });
        let k = Keycloak { issuer: format!("{}/realms/{realm}", server_url()), realm, status_secret, http };
        k.admin(reqwest::Method::POST, "", Some(body)).await;
        // The status client's service account may view users, and no more.
        let client = k.admin(reqwest::Method::GET, &format!("/clients?clientId={STATUS_CLIENT}"), None).await[0]["id"].as_str().unwrap().to_string();
        let account = k.admin(reqwest::Method::GET, &format!("/clients/{client}/service-account-user"), None).await["id"].as_str().unwrap().to_string();
        let rm = k.admin(reqwest::Method::GET, "/clients?clientId=realm-management", None).await[0]["id"].as_str().unwrap().to_string();
        let view = k.admin(reqwest::Method::GET, &format!("/clients/{rm}/roles/view-users"), None).await;
        k.admin(reqwest::Method::POST, &format!("/users/{account}/role-mappings/clients/{rm}"), Some(json!([view]))).await;
        k
    }

    /// An admin call on this realm: `path` is under `/admin/realms/{realm}`.
    async fn admin(&self, method: reqwest::Method, path: &str, body: Option<Value>) -> Value {
        let url = if path.is_empty() && method == reqwest::Method::POST {
            format!("{}/admin/realms", server_url())
        } else {
            format!("{}/admin/realms/{}{path}", server_url(), self.realm)
        };
        let mut rq = self.http.request(method.clone(), &url).bearer_auth(admin_token(&self.http).await);
        if let Some(b) = body {
            rq = rq.json(&b);
        }
        let r = rq.send().await.unwrap();
        let status = r.status();
        let text = r.text().await.unwrap_or_default();
        assert!(status.is_success(), "Keycloak admin {method} {url}: {status} {text}");
        serde_json::from_str(&text).unwrap_or(Value::Null)
    }

    async fn user_id(&self, username: &str) -> String {
        let found = self.admin(reqwest::Method::GET, &format!("/users?username={}&exact=true", username.to_lowercase()), None).await;
        found[0]["id"].as_str().unwrap_or_else(|| panic!("no user {username} in realm {}", self.realm)).to_string()
    }

    /// A person in the realm who can sign in with `password_of(username)`.
    pub async fn add_person(&self, username: &str) {
        let u = username.to_lowercase();
        self.admin(
            reqwest::Method::POST,
            "/users",
            Some(json!({
                "username": u, "enabled": true, "email": format!("{u}@example.test"), "emailVerified": true,
                "firstName": username, "lastName": "Test",
                "credentials": [{ "type": "password", "value": password_of(username), "temporary": false }],
            })),
        )
        .await;
    }

    /// Turn on two-factor for the person: an authenticator app with this
    /// secret, whose current code `code_for` gives.
    pub async fn add_code(&self, username: &str) -> String {
        let id = self.user_id(username).await;
        let secret = uuid::Uuid::new_v4().simple().to_string();
        // Keycloak takes a credential with its secret only in a whole user,
        // so the person is rewritten with their password and the app, by a
        // partial import that keeps their id: the same account, as adding an
        // authenticator app is (a new id is a new account, which the
        // appliance refuses as someone else).
        let mut user = self.admin(reqwest::Method::GET, &format!("/users/{id}"), None).await;
        user["credentials"] = json!([
            { "type": "password", "value": password_of(username), "temporary": false },
            { "type": "otp", "userLabel": "phone", "secretData": json!({ "value": secret }).to_string(),
              "credentialData": json!({ "subType": "totp", "digits": 6, "period": 30, "algorithm": "HmacSHA1", "counter": 0 }).to_string() },
        ]);
        self.admin(reqwest::Method::POST, "/partialImport", Some(json!({ "ifResourceExists": "OVERWRITE", "users": [user] }))).await;
        assert_eq!(self.user_id(username).await, id, "adding the authenticator app made a new account");
        secret
    }

    /// Make the person change their password on Keycloak's page before
    /// they can sign in, as a new account with a temporary password must.
    pub async fn require_new_password(&self, username: &str) {
        let id = self.user_id(username).await;
        self.admin(reqwest::Method::PUT, &format!("/users/{id}"), Some(json!({ "requiredActions": ["UPDATE_PASSWORD"] }))).await;
    }

    /// Put the person in exactly these groups, making any that are missing.
    pub async fn set_groups(&self, username: &str, groups: &[&str]) {
        let id = self.user_id(username).await;
        for g in self.admin(reqwest::Method::GET, &format!("/users/{id}/groups"), None).await.as_array().cloned().unwrap_or_default() {
            self.admin(reqwest::Method::DELETE, &format!("/users/{id}/groups/{}", g["id"].as_str().unwrap()), None).await;
        }
        for name in groups {
            let all = self.admin(reqwest::Method::GET, &format!("/groups?search={name}&exact=true"), None).await;
            let gid = match all.as_array().and_then(|a| a.iter().find(|g| g["name"] == *name)) {
                Some(g) => g["id"].as_str().unwrap().to_string(),
                None => {
                    self.admin(reqwest::Method::POST, "/groups", Some(json!({ "name": name }))).await;
                    let all = self.admin(reqwest::Method::GET, &format!("/groups?search={name}&exact=true"), None).await;
                    all.as_array().and_then(|a| a.iter().find(|g| g["name"] == *name)).unwrap()["id"].as_str().unwrap().to_string()
                }
            };
            self.admin(reqwest::Method::PUT, &format!("/users/{id}/groups/{gid}"), None).await;
        }
    }

    /// Give the person exactly these realm roles (besides Keycloak's own
    /// defaults), making any that are missing.
    pub async fn set_roles(&self, username: &str, roles: &[&str]) {
        let id = self.user_id(username).await;
        let mapped = self.admin(reqwest::Method::GET, &format!("/users/{id}/role-mappings/realm"), None).await;
        let ours: Vec<Value> = mapped
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|r| !r["name"].as_str().unwrap_or_default().starts_with("default-roles-") && r["name"] != "offline_access" && r["name"] != "uma_authorization")
            .collect();
        if !ours.is_empty() {
            self.admin(reqwest::Method::DELETE, &format!("/users/{id}/role-mappings/realm"), Some(Value::Array(ours))).await;
        }
        let mut add = vec![];
        for name in roles {
            let path = format!("/roles/{}", url::form_urlencoded::byte_serialize(name.as_bytes()).collect::<String>());
            let exists =
                self.http.get(format!("{}/admin/realms/{}{path}", server_url(), self.realm)).bearer_auth(admin_token(&self.http).await).send().await.unwrap().status().is_success();
            if !exists {
                self.admin(reqwest::Method::POST, "/roles", Some(json!({ "name": name }))).await;
            }
            add.push(self.admin(reqwest::Method::GET, &path, None).await);
        }
        if !add.is_empty() {
            self.admin(reqwest::Method::POST, &format!("/users/{id}/role-mappings/realm"), Some(Value::Array(add))).await;
        }
    }

    /// The usernames of everyone turned on who holds the realm role `role`
    /// directly, as the provider has them now.
    pub async fn enabled_with_role(&self, role: &str) -> Vec<String> {
        let path = format!("/roles/{}/users?first=0&max=1000&briefRepresentation=false", url::form_urlencoded::byte_serialize(role.as_bytes()).collect::<String>());
        let users = self.admin(reqwest::Method::GET, &path, None).await;
        users.as_array().into_iter().flatten().filter(|u| u["enabled"] != false).filter_map(|u| u["username"].as_str().map(String::from)).collect()
    }

    /// Another public client in the realm, for codes that are not ours.
    pub async fn add_client(&self, client_id: &str) {
        let body = json!({ "clientId": client_id, "publicClient": true, "standardFlowEnabled": true, "redirectUris": ["http://127.0.0.1/*"], "attributes": { "pkce.code.challenge.method": "S256" } });
        self.admin(reqwest::Method::POST, "/clients", Some(body)).await;
    }

    /// The admin console's client (admin console spec): confidential, the
    /// code flow with PKCE, its one redirect, proving itself with the
    /// console's key (`jwks`, from `magnus-admin jwk`), and realm roles in
    /// its ID token.
    pub async fn add_admin_client(&self, client_id: &str, redirect_uri: &str, jwks: &Value) {
        let claims = json!({ "id.token.claim": "true", "access.token.claim": "true", "userinfo.token.claim": "true", "claim.name": "realm_access.roles", "multivalued": "true" });
        let body = json!({
            "clientId": client_id, "publicClient": false, "clientAuthenticatorType": "client-jwt",
            "standardFlowEnabled": true, "directAccessGrantsEnabled": false, "serviceAccountsEnabled": false,
            "redirectUris": [redirect_uri],
            "attributes": {
                "pkce.code.challenge.method": "S256", "use.jwks.string": "true", "jwks.string": jwks.to_string(),
                "token.endpoint.auth.signing.alg": "ES256",
            },
            "protocolMappers": [{ "name": "realm roles", "protocol": "openid-connect", "protocolMapper": "oidc-usermodel-realm-role-mapper", "config": claims }],
        });
        self.admin(reqwest::Method::POST, "/clients", Some(body)).await;
    }

    /// The admin console's service account (`magnus-admin-svc`): proving
    /// itself with the console's key, holding user management in this
    /// realm and no more, as `config/keycloak-client.py --admin-console`
    /// registers it.
    pub async fn add_admin_service(&self, client_id: &str, jwks: &Value) {
        let body = json!({
            "clientId": client_id, "publicClient": false, "clientAuthenticatorType": "client-jwt",
            "standardFlowEnabled": false, "directAccessGrantsEnabled": false, "serviceAccountsEnabled": true,
            "attributes": { "use.jwks.string": "true", "jwks.string": jwks.to_string(), "token.endpoint.auth.signing.alg": "ES256" },
        });
        self.admin(reqwest::Method::POST, "/clients", Some(body)).await;
        let client = self.admin(reqwest::Method::GET, &format!("/clients?clientId={client_id}"), None).await[0]["id"].as_str().unwrap().to_string();
        let account = self.admin(reqwest::Method::GET, &format!("/clients/{client}/service-account-user"), None).await["id"].as_str().unwrap().to_string();
        let rm = self.admin(reqwest::Method::GET, "/clients?clientId=realm-management", None).await[0]["id"].as_str().unwrap().to_string();
        let mut roles = vec![];
        for r in ["manage-users", "view-users", "query-users", "view-realm", "manage-realm"] {
            roles.push(self.admin(reqwest::Method::GET, &format!("/clients/{rm}/roles/{r}"), None).await);
        }
        self.admin(reqwest::Method::POST, &format!("/users/{account}/role-mappings/clients/{rm}"), Some(Value::Array(roles))).await;
    }

    /// The realm sends its email through this SMTP server.
    pub async fn set_smtp(&self, at: std::net::SocketAddr) {
        let smtp = json!({ "host": at.ip().to_string(), "port": at.port().to_string(), "from": "id@acme.test", "fromDisplayName": "Acme sign-in", "auth": "false", "ssl": "false", "starttls": "false" });
        self.admin(reqwest::Method::PUT, "", Some(json!({ "smtpServer": smtp }))).await;
    }

    /// Whether the realm takes this password for this person now (Direct
    /// access grants on the appliance's client).
    pub async fn password_works(&self, username: &str, password: &str) -> bool {
        let r = self
            .http
            .post(format!("{}/protocol/openid-connect/token", self.issuer))
            .form(&[("grant_type", "password"), ("client_id", IDP_CLIENT), ("username", username), ("password", password), ("scope", "openid")])
            .send()
            .await
            .unwrap();
        r.status().is_success()
    }

    /// The realm's own settings, as its admin API gives them.
    pub async fn realm(&self) -> Value {
        self.admin(reqwest::Method::GET, "", None).await
    }

    /// Turn the person's account back on.
    pub async fn enable(&self, username: &str) {
        let id = self.user_id(username).await;
        self.admin(reqwest::Method::PUT, &format!("/users/{id}"), Some(json!({ "enabled": true }))).await;
    }

    /// Allow or refuse a password typed in the app ("Direct access grants")
    /// for the appliance's client.
    pub async fn set_direct_grants(&self, on: bool) {
        let mut c = self.admin(reqwest::Method::GET, &format!("/clients?clientId={IDP_CLIENT}"), None).await[0].clone();
        c["directAccessGrantsEnabled"] = json!(on);
        let id = c["id"].as_str().unwrap().to_string();
        self.admin(reqwest::Method::PUT, &format!("/clients/{id}"), Some(c)).await;
    }

    /// Remove the person from Keycloak altogether.
    pub async fn delete(&self, username: &str) {
        let id = self.user_id(username).await;
        self.admin(reqwest::Method::DELETE, &format!("/users/{id}"), None).await;
    }

    /// Turn the person's account off in Keycloak.
    pub async fn disable(&self, username: &str) {
        let id = self.user_id(username).await;
        self.admin(reqwest::Method::PUT, &format!("/users/{id}"), Some(json!({ "enabled": false }))).await;
    }

    /// A person's browser at `url`: the real Chromium signs in on Keycloak's
    /// page as `username` and follows where Keycloak sends it. The address it
    /// ended on, or Keycloak's error.
    pub async fn browser(&self, url: &str, username: &str) -> Result<String, String> {
        self.browse(url, username, false).await
    }

    /// The same, declining when Keycloak asks to grant access.
    pub async fn browser_declining(&self, url: &str, username: &str) -> Result<String, String> {
        self.browse(url, username, true).await
    }

    /// The same with a password given to them (a one-time one): they sign
    /// in with it and, asked to choose their own, keep it.
    pub async fn browser_with(&self, url: &str, username: &str, password: &str) -> Result<String, String> {
        self.browse_as(url, username, password, false).await
    }

    async fn browse(&self, url: &str, username: &str, deny: bool) -> Result<String, String> {
        self.browse_as(url, username, &password_of(username), deny).await
    }

    async fn browse_as(&self, url: &str, username: &str, password: &str, deny: bool) -> Result<String, String> {
        // A browser that hangs fails the test in two minutes, named, never
        // holds the run.
        let run = tokio::process::Command::new("node")
            .arg(workspace_root().join("tests/scratch/sign-in-browser.mjs"))
            .args([url, &username.to_lowercase(), password])
            .args(deny.then_some("deny"))
            .env("MAGNUS_CHROMIUM", crate::chromium())
            .kill_on_drop(true)
            .output();
        let Ok(out) = tokio::time::timeout(std::time::Duration::from_secs(120), run).await else {
            return Err(format!("the browser signing in at {url} did not finish in two minutes"));
        };
        let out = out.expect("run node");
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
        } else {
            Err(format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)))
        }
    }

    /// Sign in as the app does: a loopback address on this computer, the
    /// appliance's sign-in page in the person's browser, back to the loopback
    /// with a code, which the appliance exchanges. The appliance's answer.
    pub async fn app_sign_in(&self, app: &Appliance, username: &str) -> (u16, Value) {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let redirect = format!("http://127.0.0.1:{}/callback", l.local_addr().unwrap().port());
        let http = http();
        let begun: Value = http
            .post(format!("{}/v1/sign-in/browser", app.base))
            .header("x-magnus-app", "0.5.0")
            .json(&json!({ "redirect_uri": redirect, "client": "app" }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let page = begun["authorize_url"].as_str().unwrap_or_else(|| panic!("no sign-in page: {begun}")).to_string();
        let (back, browsed) = tokio::join!(loopback_once(l), self.browser(&page, username));
        browsed.unwrap_or_else(|e| panic!("the browser did not sign in: {e}"));
        let q = back;
        assert_eq!(q.get("state").map(String::as_str), begun["state"].as_str(), "the browser came back for another sign-in");
        let r = http.post(format!("{}/v1/sign-in/browser/finish", app.base)).json(&json!({ "state": q["state"], "code": q["code"], "iss": q.get("iss") })).send().await.unwrap();
        (r.status().as_u16(), r.json().await.unwrap_or(Value::Null))
    }

    /// A username and password typed in the app, and the code if given. The
    /// appliance's answer.
    pub async fn app_password(&self, app: &Appliance, username: &str, password: &str, code: Option<&str>) -> (u16, Value) {
        let r = http()
            .post(format!("{}/v1/sign-in", app.base))
            .header("x-magnus-app", "0.5.0")
            .json(&json!({ "person": username, "password": password, "code": code, "client": "app" }))
            .send()
            .await
            .unwrap();
        (r.status().as_u16(), r.json().await.unwrap_or(Value::Null))
    }

    /// Point the appliance at this realm, with more `[sign_in.oidc]`
    /// settings, and restart it. The status client's secret goes into the
    /// appliance's vault, as the owner puts it there.
    pub async fn configure_with(&self, app: &mut Appliance, more: &[(&str, &str)]) {
        let mut more: Vec<(String, String)> = more.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        for (k, v) in [("status_client", STATUS_CLIENT), ("status_credential", STATUS_CREDENTIAL)] {
            if !more.iter().any(|(m, _)| m == k) {
                more.push((k.into(), v.into()));
            }
        }
        let issuer = self.issuer.clone();
        crate::reconfigure(app, move |cfg| {
            let sign_in = cfg.entry("sign_in").or_insert(toml::Value::Table(Default::default())).as_table_mut().unwrap();
            let mut oidc = toml::Table::new();
            oidc.insert("issuer".into(), toml::Value::String(issuer));
            oidc.insert("client_id".into(), toml::Value::String(IDP_CLIENT.into()));
            oidc.insert("label".into(), toml::Value::String(IDP_LABEL.into()));
            for (k, v) in more {
                oidc.insert(k, toml::Value::String(v));
            }
            sign_in.insert("oidc".into(), toml::Value::Table(oidc));
        })
        .await;
        assert_eq!(app.api("alice").put_secret(STATUS_CREDENTIAL, &self.status_secret).await, reqwest::StatusCode::NO_CONTENT);
    }

    /// Point the appliance at this realm and restart it.
    pub async fn configure(&self, app: &mut Appliance) {
        self.configure_with(app, &[]).await
    }
}

impl Drop for Keycloak {
    fn drop(&mut self) {
        let realm = self.realm.clone();
        // Drop runs inside the test's runtime, which cannot be blocked on; the
        // cleanup gets a thread and runtime of its own.
        let _ = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
            rt.block_on(async move {
                let http = http();
                let token = admin_token(&http).await;
                let _ = http.delete(format!("{}/admin/realms/{realm}", server_url())).bearer_auth(token).send().await;
            })
        })
        .join();
    }
}

/// The query of the first request to `/callback` on this listener, answered
/// with a page that says to return to the app.
///
/// A browser that never comes back (it failed, or timed out after its two
/// minutes) fails the test in three, named, never holds the run: waited on
/// beside the browser, an unbounded accept hung five sign-in tests until the
/// job's time ran out (run 41877670131).
pub async fn loopback_once(l: tokio::net::TcpListener) -> HashMap<String, String> {
    loopback_within(l, std::time::Duration::from_secs(180)).await
}

/// `loopback_once` with its bound given: the test of the bound waits a second.
pub async fn loopback_within(l: tokio::net::TcpListener, limit: std::time::Duration) -> HashMap<String, String> {
    tokio::time::timeout(limit, callback(l)).await.unwrap_or_else(|_| panic!("the browser did not come back to the app's loopback address in {limit:?}"))
}

async fn callback(l: tokio::net::TcpListener) -> HashMap<String, String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    loop {
        let (mut s, _) = l.accept().await.unwrap();
        let mut buf = vec![0u8; 8192];
        let mut n = 0;
        while n < buf.len() {
            let k = s.read(&mut buf[n..]).await.unwrap();
            if k == 0 {
                break;
            }
            n += k;
            if buf[..n].windows(4).any(|w| w == b"\r\n\r\n") {
                break;
            }
        }
        let head = String::from_utf8_lossy(&buf[..n]).to_string();
        let target = head.lines().next().and_then(|l| l.strip_prefix("GET ")).and_then(|l| l.split(' ').next()).unwrap_or_default().to_string();
        let body = "Signed in. Return to Magnus.";
        let _ = s.write_all(format!("HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len()).as_bytes()).await;
        let Ok(u) = url::Url::parse(&format!("http://127.0.0.1{target}")) else { continue };
        if u.path() == "/callback" {
            return u.query_pairs().into_owned().collect();
        }
    }
}
