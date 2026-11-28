use super::*;

#[test]
fn a_code_goes_and_the_message_stays() {
    let out = strip("Your verification code is 482913. It expires soon.\nLunch on Friday works.");
    assert!(!out.contains("482913"), "{out}");
    assert!(out.contains("It expires soon") && out.contains("Lunch on Friday works."), "{out}");
}

/// A draft read back as one JSON line: "verification" in its body once
/// redacted the address and the date beside it (run_cf3c9786…).
#[test]
fn a_code_word_in_one_field_leaves_the_others_alone() {
    let v = serde_json::json!({ "to": "frty130@gmail.com", "date": "Wed, 11 Nov 2026 04:36:11 -0800",
        "body": "This email tests exact draft verification.\nYour verification code is 482913." });
    let out = strip(&v.to_string());
    assert!(out.contains("frty130@gmail.com") && out.contains("2026") && out.contains("-0800"), "{out}");
    assert!(out.contains("exact draft verification") && !out.contains("482913"), "{out}");
    assert!(strip("Your code is 482913, sent to frty130@gmail.com").contains("frty130@gmail.com"));
}

#[test]
fn inside_a_json_result_each_line_is_its_own() {
    let out = strip(r#"{"text":"Reset at https://a.example/reset?t=1\nOrder 482913 shipped.\nYour code is 771122."}"#);
    assert!(!out.contains("t=1") && !out.contains("771122"), "{out}");
    assert!(out.contains("Order 482913 shipped."), "{out}");
}

#[test]
fn a_number_with_nothing_about_a_code_stays() {
    assert_eq!(strip("The invoice total is 482913 pounds."), "The invoice total is 482913 pounds.");
}

#[test]
fn reset_and_sign_in_links_go_and_others_stay() {
    let out = strip("Reset: https://a.example/reset?token=abc and https://a.example/magic-login/xyz or https://a.example/blog/post");
    assert!(!out.contains("token=abc") && !out.contains("magic-login"), "{out}");
    assert!(out.contains("https://a.example/blog/post"), "{out}");
}

#[test]
fn a_link_at_the_end_of_a_bracket_is_cut_cleanly() {
    let out = strip("(see https://a.example/password/reset/1)");
    assert_eq!(out, format!("(see {LINK_REMOVED})"));
}

/// Codes and sign-in links as providers send them: each secret goes, and
/// the words around it stay.
#[test]
fn real_provider_formats_lose_their_codes_and_links() {
    #[rustfmt::skip]
    let table: &[(&str, &str, &str, &str)] = &[
        // (provider, the mail, what must go, what must stay)
        ("Google", "G-723491 is your Google verification code.", "723491", "is your Google verification code."),
        ("Microsoft", "Security code: 8172635\nIf you didn't request this code, you can ignore this email.", "8172635", "you can ignore this email."),
        ("Microsoft, short", "Use 4821 as Microsoft account security code", "4821", "as Microsoft account security code"),
        ("Apple", "Your Apple Account code is: 482913. Don't share it with anyone.", "482913", "Don't share it with anyone."),
        ("Amazon", "123456 is your Amazon OTP. Do not share it with anyone.", "123456", "is your Amazon OTP."),
        ("Stripe, hyphenated", "Your Stripe verification code is: 582-913", "582-913", "Your Stripe verification code is: "),
        ("Slack, on its own line", "Your confirmation code is below. Enter it in your open browser window.\n\n482-913\n\nThanks, Slack", "482-913", "Thanks, Slack"),
        ("spaced", "Your verification code is 482 913", "482 913", "Your verification code is "),
        ("grouped in pairs", "Your one-time passcode: 48 29 13", "48 29 13", "Your one-time passcode: "),
        ("the line after the word", "Your sign-in code:\n\n48291367\n\nIt expires in 10 minutes.", "48291367", "It expires in 10 minutes."),
        ("eleven digits and more", "Verification code: 482913672019", "482913672019", "Verification code: "),
        ("a long backup code", "Your backup code is 8F2K9QXW7M4TB3, keep it safe.", "8F2K9QXW7M4TB3", "keep it safe."),
        ("Notion, a link with no sign-in word", "Click the link below to sign in to Notion.\nhttps://www.notion.so/xyz?t=8f7d6c5b4a39281706f5e4d3c2b1a090", "8f7d6c5b4a39281706f5e4d3c2b1a090", "Click the link below to sign in to Notion."),
        ("Slack, a magic link", "Sign in to Slack\nhttps://app.slack.com/t/acme/z/7412589630-ab12cd34ef56gh78\nThis link expires in 24 hours.", "7412589630-ab12cd34ef56gh78", "This link expires in 24 hours."),
        ("Medium, a token link", "Sign in to Medium: https://medium.com/m/callback/email?token=a1b2c3d4", "a1b2c3d4", "Sign in to Medium: "),
    ];
    for (provider, mail, secret, kept) in table {
        let out = strip(mail);
        assert!(!out.contains(secret), "{provider}: {secret} reached the agent: {out}");
        assert!(out.contains(kept), "{provider}: {kept:?} was lost: {out}");
        let json = serde_json::json!({ "body": mail }).to_string();
        assert!(!strip(&json).contains(secret), "{provider}, in a JSON result: {}", strip(&json));
    }
}

/// What is not a code or a sign-in stays as written.
#[test]
fn mail_with_no_code_or_sign_in_stays_whole() {
    for mail in [
        "Order 482913 shipped.",
        "Your order 482-913 has shipped.",
        "The barcode on the box is 482913.",
        "Shipping to 482913 Main Street.",
        "Read the guide at https://docs.example.com/guide/2026-11-overview",
        "Lunch is at 12 30 on Friday.",
    ] {
        assert_eq!(strip(mail), mail);
    }
}

fn app(name: &str, url: &str, extra: serde_json::Value) -> Connector {
    let mut v = serde_json::json!({ "name": name, "url": url, "audience": "private", "label": "private+untrusted" });
    v.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    serde_json::from_value(v).unwrap()
}

/// Mail reaches the agent with its codes and sign-in links, so it can finish
/// a sign-in; only an app that asks for the filter has them taken out.
#[test]
fn mail_keeps_its_codes_unless_the_app_asks_for_the_filter() {
    let none = serde_json::json!({});
    assert!(!applies(&app("fastmail", "https://api.fastmail.com/mcp", none.clone()), "fastmail__read"), "a mail app added by its address");
    assert!(!applies(&app("work", "https://mcp.example.com/mcp", serde_json::json!({ "title": "Work Email" })), "work__read"), "titled as mail");
    assert!(!applies(&app("crm", "https://crm.example/mcp", none), "crm__read_inbox"), "a tool that reads an inbox");
    assert!(applies(&app("crm", "https://crm.example/mcp", serde_json::json!({ "filter_login_codes": true })), "crm__lookup"), "an app that asks for it");
    assert!(!applies(&app("gmail", "https://gmail.example/mcp", serde_json::json!({ "filter_login_codes": true, "keep_login_codes": true })), "gmail__read"), "the opt-out");
    let catalog = crate::catalog::manifests().iter().find(|m| m.category == "email").map(crate::catalog::connector).unwrap();
    assert!(!applies(&catalog, &format!("{}__read", catalog.name)), "{}: the catalog's email app", catalog.name);
}
