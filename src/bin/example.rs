use smtp_rs::{Broker, Result};

fn main() -> Result<()> {
    println!("SMTP Protocol Example (RFC 821)");

    let broker = Broker::new("127.0.0.1:2525")?;
    broker.accept()?;

    Ok(())
}
