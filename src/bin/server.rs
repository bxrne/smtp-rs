use smtp_rs::{Broker, Mail, Result, Transport};
use std::sync::Arc;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Default)]
struct StdoutTransport;

impl Transport for StdoutTransport {
    fn deliver(&self, mail: Mail) -> Result<()> {
        info!(
            from = %mail.from,
            to = ?mail.to,
            body = %mail.body,
            "accepted mail"
        );
        Ok(())
    }
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .try_init();
}

fn main() -> Result<()> {
    init_tracing();
    let addr = "127.0.0.1:2525";
    info!(%addr, "SMTP server listening");
    let broker = Broker::new_with_transport(addr, Arc::new(StdoutTransport))?;
    broker.accept()
}

#[cfg(test)]
mod tests {
    use super::*;

    // GIVEN a mail message WHEN delivered through StdoutTransport THEN it returns Ok
    #[test]
    fn test_stdout_transport_deliver_ok() {
        let transport = StdoutTransport;
        let mail = Mail {
            from: "sender@example.com".into(),
            to: vec!["recipient@example.com".into()],
            body: "hello".into(),
        };
        assert!(transport.deliver(mail).is_ok());
    }

    // GIVEN a StdoutTransport WHEN used as a trait object THEN it can be shared via Arc
    #[test]
    fn test_stdout_transport_is_object_safe() {
        let transport: Arc<dyn Transport> = Arc::new(StdoutTransport);
        let mail = Mail {
            from: "a@b".into(),
            to: vec!["c@d".into()],
            body: String::new(),
        };
        assert!(transport.deliver(mail).is_ok());
    }

    // GIVEN a free local port WHEN the server's broker construction is exercised
    // THEN it binds successfully with the StdoutTransport
    #[test]
    fn test_server_broker_binds_with_stdout_transport() {
        let broker = Broker::new_with_transport("127.0.0.1:0", Arc::new(StdoutTransport))
            .expect("broker should bind");
        let addr = broker.local_addr().expect("should have local addr");
        assert_eq!(addr.ip().to_string(), "127.0.0.1");
        assert!(addr.port() > 0);
    }

    // GIVEN init_tracing is called twice WHEN the second call runs
    // THEN it does not panic because try_init swallows the already-set error
    #[test]
    fn test_init_tracing_is_idempotent() {
        init_tracing();
        init_tracing();
    }
}
