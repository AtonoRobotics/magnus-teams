//! Sending the appliance's account email: password recovery, invitations.
//! One SMTP server from `[mail]`, credentials in the vault; the message is
//! plain text. Answers whether the email went out: with no `[mail]`, sent
//! nowhere, so the caller can say so (an admin-relayed link instead of a
//! claim).

use anyhow::{Context, Result};
use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

pub async fn send(k: &crate::kernel::Kernel, to: &str, subject: &str, text: String) -> Result<bool> {
    let Some(m) = &k.cfg.mail else { return Ok(false) };
    let creds: Option<Credentials> = match (&m.username, &m.credential) {
        (Some(user), Some(credential)) => {
            let secret = k.vault.get(credential)?.ok_or_else(|| anyhow::anyhow!("the vault holds no secret named {credential}"))?;
            Some(Credentials::new(user.clone(), secret.expose().to_string()))
        }
        (None, None) => None,
        _ => anyhow::bail!("mail.username and mail.credential are set together, or neither"),
    };
    let from: lettre::message::Mailbox = m.from.parse().with_context(|| format!("mail.from {} does not parse", m.from))?;
    let to_box: lettre::message::Mailbox = to.parse().with_context(|| format!("{to} does not parse as an address"))?;
    let msg = Message::builder().from(from).to(to_box).subject(subject).header(ContentType::TEXT_PLAIN).body(text)?;
    let transport = match m.tls.as_str() {
        "tls" => {
            let b = AsyncSmtpTransport::<Tokio1Executor>::relay(&m.host)?.port(m.port);
            match creds {
                Some(c) => b.credentials(c).build(),
                None => b.build(),
            }
        }
        "none" => {
            let b = AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&m.host).port(m.port);
            match creds {
                Some(c) => b.credentials(c).build(),
                None => b.build(),
            }
        }
        _ => {
            let b = AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&m.host)?.port(m.port);
            match creds {
                Some(c) => b.credentials(c).build(),
                None => b.build(),
            }
        }
    };
    AsyncTransport::send(&transport, msg).await?;
    Ok(true)
}
