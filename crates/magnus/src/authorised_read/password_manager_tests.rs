//! The password manager's login read goes through the one authorised read:
//! someone the app is not for is refused before anything goes out.

use super::Refused;
use crate::admin_app::tests::server_with;
use crate::kernel::NewRun;
use axum::Router;
use serde_json::json;

const MORE: &str = "[computer]\nsocket = \"/nonexistent/computerd.sock\"\npassword_manager = \"pm\"\n\
                    [[connectors]]\nname = \"pm\"\nurl = \"https://pm.example/mcp\"\naudience = \"internal\"\nlabel = \"private\"\naccess_groups = [\"it\"]\n\
                    [[connectors]]\nname = \"crm\"\nurl = \"https://crm.example/mcp\"\naudience = \"internal\"\nlabel = \"private\"\n";

#[tokio::test(flavor = "multi_thread")]
async fn the_password_manager_read_is_authorised_as_any_read() {
    let s = server_with(MORE, |_| Router::new()).await;
    let (_, run) = s.k.create_run("person:mo", "mo", "sign in to the shop".into(), vec![], NewRun::default()).await.unwrap();
    let site = json!({ "site": "shop.example" });
    let r = s.k.password_manager_read(&run, "mo", "pm", "get_login", site.clone()).await;
    assert!(matches!(r, Err(Refused::NoRights)), "the password manager was read for someone it is not for: {r:?}");
    // Only the configured password manager answers a login read.
    let other = s.k.password_manager_read(&run, "mo", "crm", "get_login", site).await;
    assert!(matches!(other, Err(Refused::Unknown)), "{other:?}");
    s.done().await;
}
