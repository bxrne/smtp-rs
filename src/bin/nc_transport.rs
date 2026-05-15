use smtp_rs::{Broker, Mail, Result, Transport};
use std::sync::Arc;

#[derive(Debug, Default)]
struct StdoutTransport;

impl Transport for StdoutTransport {
    fn deliver(&self, mail: Mail) -> Result<()> {
        println!("--- accepted mail ---");
        println!("from: {}", mail.from);
        println!("to: {:?}", mail.to);
        println!("body:\n{}", mail.body);
        Ok(())
    }
}

fn main() -> Result<()> {
    println!("SMTP server listening on 127.0.0.1:2525");
    let broker = Broker::new_with_transport("127.0.0.1:2525", Arc::new(StdoutTransport))?;
    broker.accept()
}
