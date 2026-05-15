//! Network tooling for the lib

use crate::Error;
use crate::Result;
use std::io::Write;
use std::net::{TcpListener, TcpStream};

/// Handler for individual TCP sessions.
pub struct Session {
    stream: TcpStream,
}

impl Session {
    /// Create a new session wrapping the given stream.
    pub fn new(stream: TcpStream) -> Self {
        Session { stream }
    }

    /// Handle the session by reading commands and writing replies.
    pub fn handle(&mut self) -> Result<()> {
        self.stream
            .write_all(b"220 Service ready\r\n")
            .map_err(|e| Error::SessionError(e.to_string()))?;

        // TODO: Loop and dispatch commands until the session is closed

        Ok(())
    }
}

/// Broker for TCP connections and sessions.
pub struct Broker {
    listener: TcpListener,
}

impl Broker {
    /// Create a new broker listening on the specified address.
    pub fn new(addr: &str) -> Result<Self> {
        let listener = TcpListener::bind(addr).map_err(|e| Error::SessionError(e.to_string()))?;
        Ok(Broker { listener })
    }

    /// Return the local socket address the broker is bound to.
    pub fn local_addr(&self) -> Result<std::net::SocketAddr> {
        self.listener
            .local_addr()
            .map_err(|e| Error::SessionError(e.to_string()))
    }

    /// Accept incoming connections and handle sessions.
    pub fn accept(&self) -> Result<()> {
        for stream in self.listener.incoming() {
            match stream {
                Ok(stream) => {
                    let mut session = Session::new(stream);
                    session.handle()?;
                }
                Err(e) => return Err(Error::SessionError(e.to_string())),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::net::TcpStream;
    use std::thread;
    use std::time::Duration;

    // GIVEN a free local port WHEN a Broker is created THEN it binds successfully
    #[test]
    fn test_broker_binds_to_address() {
        let broker = Broker::new("127.0.0.1:0").expect("broker should bind");
        let addr = broker.local_addr().expect("should have local addr");
        assert_eq!(addr.ip().to_string(), "127.0.0.1");
        assert!(addr.port() > 0);
    }

    // GIVEN an address already in use WHEN a Broker is created THEN it returns a SessionError
    #[test]
    fn test_broker_bind_failure_returns_session_error() {
        let first = Broker::new("127.0.0.1:0").expect("broker should bind");
        let addr = first.local_addr().expect("should have local addr");

        match Broker::new(&addr.to_string()) {
            Err(Error::SessionError(_)) => {}
            Err(other) => panic!("expected SessionError, got {}", other),
            Ok(_) => panic!("expected SessionError, got Ok"),
        }
    }

    // GIVEN a malformed address WHEN a Broker is created THEN it returns a SessionError
    #[test]
    fn test_broker_invalid_address_returns_session_error() {
        match Broker::new("not-a-valid-address") {
            Err(Error::SessionError(_)) => {}
            Err(other) => panic!("expected SessionError, got {}", other),
            Ok(_) => panic!("expected SessionError, got Ok"),
        }
    }

    // GIVEN a running broker WHEN a client connects THEN it receives "220 Service ready"
    #[test]
    fn test_session_replies_with_service_ready() {
        let broker = Broker::new("127.0.0.1:0").expect("broker should bind");
        let addr = broker.local_addr().expect("should have local addr");

        let handle = thread::spawn(move || {
            // Accept a single connection and handle it.
            let (stream, _) = broker
                .listener
                .accept()
                .expect("listener should accept connection");
            let mut session = Session::new(stream);
            session.handle().expect("session should handle");
        });

        let mut client = TcpStream::connect(addr).expect("client should connect");
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("read timeout should set");

        let mut buf = [0u8; 64];
        let n = client.read(&mut buf).expect("client should read greeting");
        let greeting = std::str::from_utf8(&buf[..n]).expect("greeting should be utf-8");
        assert_eq!(greeting, "220 Service ready\r\n");

        handle.join().expect("session thread should join");
    }
}
