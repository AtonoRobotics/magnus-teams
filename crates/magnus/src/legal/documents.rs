//! The agreements' text, as this server states them: the Terms of Use, the
//! Privacy notice, the Tester Terms and the usage-data prompt.
//!
//! The Terms of Use and Privacy notice are adapted from Basecamp's open
//! policies (github.com/basecamp/policies, © 37signals LLC, CC BY 4.0); the
//! Tester Terms adapt the same base, since no openly licensed tester
//! agreement fit. Each says so in its attribution line. They state only
//! what this code does: who sees what is the code's rule (`team`,
//! `anonymize`, `admin_app`), and every fact the code cannot know comes
//! from the `[legal]` config.
//!
//! The model (owner, 2026-11-11): outside a team each person is a private
//! party; anything put in a team workspace is the team's to see, its Team
//! Admin owns and moderates it, and teams are isolated from each other; the
//! Magnus admin supports accounts and runs the server and does not see
//! anyone's data.

use super::Settings;
use serde::Serialize;

/// The date each text was last written. With the digest of the text as
/// rendered, it makes the version people accept.
const TERMS_DATE: &str = "2026-11-11";
const PRIVACY_DATE: &str = "2026-11-11";
const TESTERS_DATE: &str = "2026-11-11";
const USAGE_DATE: &str = "2026-11-11";
/// Hex characters of a text's digest in its version.
const DIGEST_HEX: usize = 8;

/// Where the documents are served, without signing in.
pub const TERMS_PATH: &str = "/legal/terms";
pub const PRIVACY_PATH: &str = "/legal/privacy";
pub const TESTERS_PATH: &str = "/legal/testers";

const BASECAMP: &str = "Adapted from the Basecamp policies (https://github.com/basecamp/policies), © 37signals LLC, used under the Creative Commons Attribution 4.0 International license (https://creativecommons.org/licenses/by/4.0/). Changed to describe this Magnus server.";
const BASECAMP_TESTERS: &str = "Written on the base of the Basecamp policies (https://github.com/basecamp/policies), © 37signals LLC, used under the Creative Commons Attribution 4.0 International license (https://creativecommons.org/licenses/by/4.0/). No openly licensed tester agreement fit, so this one is new text in the same plain style.";

#[derive(Serialize, Clone, Debug)]
pub struct Section {
    pub heading: String,
    pub paragraphs: Vec<String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct Doc {
    pub id: &'static str,
    pub title: &'static str,
    pub version: String,
    pub path: &'static str,
    pub sections: Vec<Section>,
    pub attribution: &'static str,
}

/// The usage-data prompt: one question and two answers of equal weight.
#[derive(Serialize, Clone, Debug)]
pub struct Prompt {
    pub version: String,
    pub title: &'static str,
    pub text: Vec<String>,
    pub yes: &'static str,
    pub no: &'static str,
}

/// The version of each text as this server renders it now.
#[derive(Clone, Debug, PartialEq)]
pub struct Versions {
    pub terms: String,
    pub privacy: String,
    pub testers: String,
    pub usage: String,
}

impl Versions {
    pub fn of(l: &Settings) -> Versions {
        Versions { terms: terms(l).version, privacy: privacy(l).version, testers: testers(l).version, usage: usage_prompt(l).version }
    }
}

fn s(heading: &str, paragraphs: &[&str]) -> Section {
    Section { heading: heading.into(), paragraphs: paragraphs.iter().map(|p| p.to_string()).collect() }
}

fn version(date: &str, parts: &impl Serialize) -> String {
    use sha2::Digest;
    let bytes = serde_json::to_vec(parts).unwrap_or_default();
    format!("{date}.{}", &hex::encode(sha2::Sha256::digest(&bytes))[..DIGEST_HEX])
}

fn doc(id: &'static str, title: &'static str, date: &str, path: &'static str, sections: Vec<Section>, attribution: &'static str) -> Doc {
    let v = version(date, &(&sections, attribution));
    Doc { id, title, version: v, path, sections, attribution }
}

/// The Terms of Use.
pub fn terms(l: &Settings) -> Doc {
    let who = &l.operator;
    let mail = &l.contact;
    let sections = vec![
        s("Who these terms are with", &[
            &format!("This Magnus server is run by {who} (\"we\", \"us\"). Magnus is an AI agent that works for you on a computer of its own on this server: it chats with you, does tasks, uses the apps you connect, and works with your team in shared workspaces."),
            "By using Magnus you agree to these terms and to the Privacy notice beside them. If you do not agree, do not use Magnus: sign out instead.",
            &format!("Questions about these terms go to {mail}."),
        ]),
        s("Your account", &[
            "You are responsible for keeping your password and your devices safe, and for what is done with your account.",
            "An account is for one person. Do not share it.",
        ]),
        s("What you put in stays yours", &[
            "What you write, upload or connect stays yours. We store and process it only to run Magnus for you.",
            "Outside a team workspace you are a private party: your chats, tasks, files, memory, rules and connected apps are yours alone. Nobody else on this server sees them unless you accept another agreement that says otherwise (the Tester Terms) or you share them in a team workspace.",
        ]),
        s("Team workspaces are shared", &[
            "A team workspace is a shared space. Anything posted or shared in a team workspace is visible to every member of that team.",
            "Each team workspace is owned by its Team Admin, who can moderate it and delete anything in it there.",
            "Sharing something into a workspace puts a copy there. You keep your own original, and removing the copy from the workspace does not touch it.",
            "Teams are isolated from each other: nothing in one team is visible to another team or to anyone outside it.",
        ]),
        s("What the Magnus admin does", &[
            "The Magnus admin supports people's accounts and runs this server: inviting people, resetting passwords, turning accounts off, removing people, the server's settings and the organisation's connectors, and, for the server's safety, pausing a computer or stopping a task.",
            "The Magnus admin does not see your data or any team's content. Support that needs your data happens only in a session you grant.",
        ]),
        s("Magnus acts for you", &[
            "Magnus can send, buy, change and delete things in the apps you connect, within the rules you and your organisation set, and it asks first where those rules say so. It is software and can be wrong. Check what matters before you rely on it.",
            "You are responsible for what you ask Magnus to do, and for following the terms of the apps you connect to it.",
        ]),
        s("Acceptable use", &[
            "Do not use Magnus to break the law, to harm or harass anyone, to reach data that is not yours, or to attack this server or anyone else's. We may turn off an account that does.",
        ]),
        s("Developers and usage data", &[
            "By default Magnus's developers see none of your work. The app asks whether you want to help improve Magnus by sharing anonymous usage data; you can say no, and change your answer in Settings at any time. The Privacy notice says what that data holds.",
            "Joining the tester program is a separate agreement, the Tester Terms, which you accept or not on its own.",
        ]),
        s("No warranty", &[
            "We provide Magnus \"as is\" and \"as available\", without warranties of any kind. We do not promise that it will be uninterrupted, error-free or right. Your use of Magnus is at your own risk.",
            "To the extent the law allows, we are not liable for indirect, incidental or consequential damages, or for lost profits, data or goodwill, arising from your use of Magnus.",
        ]),
        s("Changes and ending", &[
            "When these terms change, their version changes and the app asks you to accept the new version before you go on.",
            "You may stop using Magnus at any time, and ask in Settings › Data and privacy for your data to be deleted. An account can be turned off or removed by the Magnus admin.",
        ]),
        s("Governing law", &[&format!("These terms are governed by the law of {}.", l.governing_law)]),
    ];
    doc("terms", "Terms of Use", TERMS_DATE, TERMS_PATH, sections, BASECAMP)
}

/// The Privacy notice.
pub fn privacy(l: &Settings) -> Doc {
    let who = &l.operator;
    let mail = &l.contact;
    let sections = vec![
        s("Who is responsible", &[&format!("This Magnus server is run by {who}. Write to {mail} with any question about your data.")]),
        s("What we keep and why", &[
            "We keep only what Magnus needs to work for you.",
            "Your account: your name, your email address and your password, which is stored only as a hash.",
            "Your work: your chats, tasks, files, memory, rules and what Magnus did for you, in a log of every step, so you can see what happened and Magnus can pick up where it left off.",
            "Your connected apps: their sign-ins, kept encrypted in the server's vault.",
            "Usage records: when things ran, how long they took, what they cost, which model and tools ran and what went wrong. These hold no words of yours.",
        ]),
        s("Who sees what", &[
            "You see everything of yours.",
            "Outside a team workspace, nobody else on this server sees your work. Anything you post or share in a team workspace is seen by that team's members, and its Team Admin can moderate and delete it there. Teams do not see each other.",
            "The Magnus admin runs the server and supports accounts, and does not see your data or any team's content. Support that needs your data happens only in a session you grant.",
            "To do your work, Magnus sends what a task needs to the AI model provider this server uses, and to the apps you connect, under their own terms.",
        ]),
        s("Magnus's developers", &[
            &format!("Magnus's developers on this server are the people {who} gives the developer role. By default they see none of your work."),
            "If you say yes to sharing anonymous usage data, they see your usage records with every name and id replaced by a code they cannot turn back into you: kinds of events, times, counts, costs, tokens, models, tool names, kinds of errors and the names of the sites Magnus reached. They never see your words, tool inputs or results, file names, the paths of web addresses, memory or rules that way.",
            "If you say no, nothing of yours reaches them, not even anonymised. You can change your answer in Settings at any time, and it applies from then on.",
            "If you join the tester program, the Tester Terms apply: developers see your work in full.",
            "Problem reports you choose to send are separate; Settings › Data and privacy says what they carry.",
        ]),
        s("How long we keep it", &[
            "Your work is kept until you delete it. What you delete waits in the trash for the time the Magnus admin sets, at most 30 days, so a mistake can be undone; then it is removed, and it leaves the backups within 30 days.",
            "Usage records are part of the server's permanent log and are kept for as long as the server runs. They hold none of your words.",
        ]),
        s("Your rights", &[
            "You can see your data in the app, ask for it to be deleted in Settings › Data and privacy (the server's owner has 30 days to act), change your usage-data answer, and leave the tester program, at any time.",
            &format!("For anything else about your data, write to {mail}."),
        ]),
        s("Security", &["Secrets such as your connected apps' sign-ins are encrypted in the server's vault; passwords are stored only as hashes. Every action Magnus takes is recorded in the log."]),
        s("Changes", &["When this notice changes, its version changes and the app shows you the new one before you go on."]),
    ];
    doc("privacy", "Privacy notice", PRIVACY_DATE, PRIVACY_PATH, sections, BASECAMP)
}

/// The Tester Terms.
pub fn testers(l: &Settings) -> Doc {
    let who = &l.operator;
    let mail = &l.contact;
    let days = l.tester_copy_days;
    let sections = vec![
        s("What the tester program is", &[
            &format!("Testers help {who} find and fix problems in Magnus by letting Magnus's developers see their work in full. Joining is your choice, and you can leave at any time."),
            "Magnus is test software. It may fail, lose work or act wrongly. Keep your own copy of anything that matters and check what Magnus does.",
        ]),
        s("What developers see", &[
            "While you are a tester, Magnus's developers may see your data in full to find and fix bugs: your conversations with Magnus, your tasks, every tool's inputs and results, the files your tasks touch, the web addresses Magnus reached, and the screens of your Magnus computer during tasks.",
            "Anonymous usage data is collected from you too, as the Privacy notice describes.",
            "What you share in a team workspace stays as the Terms of Use say; joining does not show your teams' content to anyone outside them.",
        ]),
        s("Who the developers are", &[&format!("The developers are the people {who}, who runs this server, gives the developer role. They use what they see only to find and fix problems in Magnus.")]),
        s("How long it is kept", &[
            "Developers read your data where it already is on this server; the server makes no separate copy for them. It is kept as the Privacy notice says.",
            &format!("Anything a developer copies out of it to fix a bug, such as an excerpt in a bug report, is kept for at most {days} days."),
        ]),
        s("Leaving", &[
            "Leave the program in Settings at any time. From then on developers no longer see your work in full; your answer about anonymous usage data applies again.",
            &format!("What developers already saw while you were a tester cannot be unseen; copies they made are deleted within {days} days of being made. To ask about them, write to {mail}."),
        ]),
        s("Changes", &["When these terms change, you are not seen in full until you accept the new version."]),
    ];
    doc("testers", "Tester Terms", TESTERS_DATE, TESTERS_PATH, sections, BASECAMP_TESTERS)
}

/// The usage-data prompt.
pub fn usage_prompt(_l: &Settings) -> Prompt {
    let title = "Help improve Magnus";
    let text = vec![
        "You can help improve Magnus by sharing anonymous usage data with its developers: when things ran, how long they took, what they cost, and which models and tools were used, with every name and id replaced by a code they cannot turn back into you.".to_string(),
        "Never your words, files, tool inputs or results, or the pages you visit. If you say no, nothing of yours reaches them. You can change this in Settings at any time.".to_string(),
    ];
    let (yes, no) = ("Share anonymous usage data", "Don't share");
    Prompt { version: version(USAGE_DATE, &(title, &text, yes, no)), title, text, yes, no }
}

fn escape(t: &str) -> String {
    t.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// A document as a page of its own.
pub fn html(d: &Doc) -> String {
    let mut out = format!("<h1>{}</h1><p><small>Version {}</small></p>", escape(d.title), escape(&d.version));
    for sec in &d.sections {
        out.push_str(&format!("<h2>{}</h2>", escape(&sec.heading)));
        for p in &sec.paragraphs {
            out.push_str(&format!("<p>{}</p>", escape(p)));
        }
    }
    out.push_str(&format!("<hr><p><small>{}</small></p>", escape(d.attribution)));
    out
}
