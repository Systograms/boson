//! SMTP implementation of [`boson_ports::Mailer`].
//!
//! Supports developer-owned SMTP relays (including SES SMTP and Mailgun SMTP).
//! Boson never creates mail providers. TLS is required by default.

use std::time::Instant;

use async_trait::async_trait;
use boson_kernel::MailConfig;
use boson_ports::{Email, HealthCheck, HealthStatus, Mailer, PortError};
use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
    message::Mailbox,
    transport::smtp::{
        authentication::Credentials,
        client::{Tls, TlsParameters},
    },
};

/// An SMTP [`Mailer`].
#[derive(Clone)]
pub struct SmtpMailer {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    host: String,
}

impl SmtpMailer {
    /// Builds a transport from typed platform mail configuration.
    ///
    /// # Errors
    ///
    /// Returns [`PortError::Invalid`] for incomplete config or unsafe TLS choices.
    pub fn from_config(config: &MailConfig) -> Result<Self, PortError> {
        let settings = validate_config(config)?;
        let mut builder = match settings.tls.as_str() {
            "starttls" => {
                let tls = TlsParameters::new(settings.host.clone())
                    .map_err(|error| PortError::Invalid(error.to_string()))?;
                AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&settings.host)
                    .map_err(|error| PortError::Invalid(error.to_string()))?
                    .port(settings.port)
                    .tls(Tls::Required(tls))
            }
            "tls" => {
                let tls = TlsParameters::new(settings.host.clone())
                    .map_err(|error| PortError::Invalid(error.to_string()))?;
                AsyncSmtpTransport::<Tokio1Executor>::relay(&settings.host)
                    .map_err(|error| PortError::Invalid(error.to_string()))?
                    .port(settings.port)
                    .tls(Tls::Wrapper(tls))
            }
            "none" => AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&settings.host)
                .port(settings.port)
                .tls(Tls::None),
            other => {
                return Err(PortError::Invalid(format!(
                    "unsupported mail.tls `{other}`; expected starttls, tls, or none"
                )));
            }
        };

        if !settings.username.is_empty() {
            builder = builder.credentials(Credentials::new(
                settings.username.clone(),
                settings.password.clone(),
            ));
        }

        Ok(Self {
            transport: builder.build(),
            host: settings.host,
        })
    }
}

#[derive(Debug, Clone)]
struct SmtpSettings {
    host: String,
    port: u16,
    username: String,
    password: String,
    tls: String,
}

fn validate_config(config: &MailConfig) -> Result<SmtpSettings, PortError> {
    if config.from.trim().is_empty() {
        return Err(PortError::Invalid(
            "mail.from is required for the smtp provider".into(),
        ));
    }
    if config.host.trim().is_empty() {
        return Err(PortError::Invalid(
            "mail.host is required for the smtp provider".into(),
        ));
    }
    let tls = config.tls.trim().to_ascii_lowercase();
    if tls.is_empty() {
        return Err(PortError::Invalid("mail.tls must not be empty".into()));
    }
    if tls == "none" && !is_loopback_host(config.host.trim()) {
        return Err(PortError::Invalid(
            "mail.tls=none is only allowed for loopback SMTP hosts".into(),
        ));
    }
    Ok(SmtpSettings {
        host: config.host.trim().to_owned(),
        port: config.port,
        username: config.username.clone(),
        password: config.password.clone(),
        tls,
    })
}

fn is_loopback_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

fn build_message(email: &Email) -> Result<Message, PortError> {
    if email.to.trim().is_empty()
        || email.from.trim().is_empty()
        || email.subject.trim().is_empty()
        || email.idempotency_key.trim().is_empty()
    {
        return Err(PortError::Invalid(
            "email recipient, sender, subject, and idempotency key are required".into(),
        ));
    }
    let to: Mailbox = email
        .to
        .parse()
        .map_err(|error| PortError::Invalid(format!("invalid mail.to: {error}")))?;
    let from: Mailbox = email
        .from
        .parse()
        .map_err(|error| PortError::Invalid(format!("invalid mail.from: {error}")))?;
    Message::builder()
        .from(from)
        .to(to)
        .subject(&email.subject)
        .message_id(Some(format!("<{}@boson>", email.idempotency_key)))
        .body(email.text.clone())
        .map_err(|error| PortError::Invalid(error.to_string()))
}

#[async_trait]
impl Mailer for SmtpMailer {
    async fn send(&self, email: Email) -> Result<(), PortError> {
        let message = build_message(&email)?;
        self.transport
            .send(message)
            .await
            .map(|_| ())
            .map_err(|error| PortError::Provider(error.to_string()))
    }
}

#[async_trait]
impl HealthCheck for SmtpMailer {
    async fn check(&self) -> HealthStatus {
        let started = Instant::now();
        match self.transport.test_connection().await {
            Ok(true) => HealthStatus {
                component: "mailer-smtp".into(),
                healthy: true,
                message: Some(format!("smtp host `{}` reachable", self.host)),
                latency_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            },
            Ok(false) => HealthStatus {
                component: "mailer-smtp".into(),
                healthy: false,
                message: Some(format!(
                    "smtp host `{}` rejected connection test",
                    self.host
                )),
                latency_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            },
            Err(error) => HealthStatus {
                component: "mailer-smtp".into(),
                healthy: false,
                message: Some(error.to_string()),
                latency_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_host_and_from() {
        let mut config = MailConfig {
            provider: "smtp".into(),
            ..MailConfig::default()
        };
        config.host = String::new();
        assert!(validate_config(&config).is_err());
        config.host = "smtp.example.com".into();
        config.from = "Boson <no-reply@example.com>".into();
        assert!(validate_config(&config).is_ok());
    }

    #[test]
    fn rejects_cleartext_for_remote_hosts() {
        let config = MailConfig {
            provider: "smtp".into(),
            host: "smtp.example.com".into(),
            from: "Boson <no-reply@example.com>".into(),
            tls: "none".into(),
            ..MailConfig::default()
        };
        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn allows_cleartext_on_loopback() {
        let config = MailConfig {
            provider: "smtp".into(),
            host: "127.0.0.1".into(),
            from: "Boson <no-reply@localhost>".into(),
            tls: "none".into(),
            ..MailConfig::default()
        };
        assert!(validate_config(&config).is_ok());
    }

    #[test]
    fn builds_message_with_idempotency_header() {
        let message = build_message(&Email {
            to: "user@example.com".into(),
            from: "Boson <no-reply@example.com>".into(),
            subject: "Hello".into(),
            text: "body".into(),
            idempotency_key: "evt-1".into(),
        })
        .unwrap();
        let formatted = String::from_utf8(message.formatted()).unwrap();
        assert!(formatted.contains("evt-1@boson"));
    }

    #[tokio::test]
    async fn builds_starttls_transport() {
        let config = MailConfig {
            provider: "smtp".into(),
            host: "smtp.example.com".into(),
            port: 587,
            username: "user".into(),
            password: "pass".into(),
            tls: "starttls".into(),
            from: "Boson <no-reply@example.com>".into(),
            ..MailConfig::default()
        };
        // Construction is sync, but Drop of lettre's async pool needs a runtime.
        let mailer = SmtpMailer::from_config(&config).unwrap();
        assert_eq!(mailer.host, "smtp.example.com");
    }
}
