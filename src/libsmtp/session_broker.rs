//! Network tooling: TCP broker and per-connection session driver.
//!
//! The session is a thin transport wrapper around [`Machine`]: it reads one
//! line at a time, feeds it to the state machine, and writes any replies
//! back to the client. All protocol logic lives in [`crate::libsmtp::model`].

use crate::Error;
use crate::Result;
use crate::libsmtp::model::{Machine, Reply};
use crate::libsmtp::transport::{NullTransport, Transport};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;
use tracing::{debug, warn};

/// Handler for individual TCP sessions.
pub struct Session {
    stream: TcpStream,
    transport: Arc<dyn Transport>,
}

impl Session {
    /// Create a new session wrapping the given stream.
    pub fn new(stream: TcpStream, transport: Arc<dyn Transport>) -> Self {
        Session { stream, transport }
    }

    /// Drive the SMTP state machine over this connection until the client
    /// issues `QUIT` or the connection is closed.
    pub fn handle(&mut self) -> Result<()> {
        let mut machine = Machine::new();
        let greeting = machine.greet();
        self.write_reply(&greeting)?;

        let read_stream = self
            .stream
            .try_clone()
            .map_err(|e| Error::SessionError(e.to_string()))?;
        let reader = BufReader::new(read_stream);

        for line in reader.lines() {
            let line = line.map_err(|e| Error::SessionError(e.to_string()))?;
            let outcome = machine.step_with_mail(&line);
            if let Some(mail) = outcome.accepted {
                self.transport.deliver(mail)?;
            }
            if let Some(reply) = outcome.reply {
                self.write_reply(&reply)?;
            }
            if machine.is_closed() {
                break;
            }
        }

        Ok(())
    }

    fn write_reply(&mut self, reply: &Reply) -> Result<()> {
        self.stream
            .write_all(reply.format().as_bytes())
            .map_err(|e| Error::SessionError(e.to_string()))
    }
}

/// Broker for TCP connections and sessions.
pub struct Broker {
    listener: TcpListener,
    transport: Arc<dyn Transport>,
}

impl Broker {
    /// Create a new broker listening on the specified address.
    pub fn new(addr: &str) -> Result<Self> {
        let listener = TcpListener::bind(addr).map_err(|e| Error::SessionError(e.to_string()))?;
        Ok(Broker {
            listener,
            transport: Arc::new(NullTransport),
        })
    }

    /// Create a new broker with a custom transport.
    pub fn new_with_transport(addr: &str, transport: Arc<dyn Transport>) -> Result<Self> {
        let listener = TcpListener::bind(addr).map_err(|e| Error::SessionError(e.to_string()))?;
        Ok(Broker {
            listener,
            transport,
        })
    }

    /// Return the local socket address the broker is bound to.
    pub fn local_addr(&self) -> Result<std::net::SocketAddr> {
        self.listener
            .local_addr()
            .map_err(|e| Error::SessionError(e.to_string()))
    }

    /// Accept incoming connections and handle each session on its own thread.
    ///
    /// Per-session errors are logged via `tracing::warn` and do not stop the
    /// broker; only failures of the accept call itself are fatal.
    pub fn accept(&self) -> Result<()> {
        for stream in self.listener.incoming() {
            match stream {
                Ok(stream) => {
                    let peer = stream
                        .peer_addr()
                        .map(|a| a.to_string())
                        .unwrap_or_else(|_| "<unknown>".to_string());
                    let transport = self.transport.clone();
                    debug!(%peer, "accepted connection");
                    thread::Builder::new()
                        .name(format!("smtp-session-{peer}"))
                        .spawn(move || {
                            let mut session = Session::new(stream, transport);
                            if let Err(e) = session.handle() {
                                warn!(%peer, error = %e, "session ended with error");
                            } else {
                                debug!(%peer, "session ended");
                            }
                        })
                        .map_err(|e| Error::SessionError(e.to_string()))?;
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
    use std::io::{BufRead, BufReader, Read, Write};
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

    fn spawn_one_shot_broker() -> (std::net::SocketAddr, thread::JoinHandle<Result<()>>) {
        let broker = Broker::new("127.0.0.1:0").expect("broker should bind");
        let addr = broker.local_addr().expect("should have local addr");
        let handle = thread::spawn(move || {
            let (stream, _) = broker
                .listener
                .accept()
                .map_err(|e| Error::SessionError(e.to_string()))?;
            let mut session = Session::new(stream, broker.transport.clone());
            session.handle()
        });
        (addr, handle)
    }

    // GIVEN a running broker WHEN a client connects THEN it receives "220 Service ready"
    #[test]
    fn test_session_replies_with_service_ready_on_connect() {
        let (addr, handle) = spawn_one_shot_broker();

        let mut client = TcpStream::connect(addr).expect("client should connect");
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("read timeout should set");

        let mut buf = [0u8; 64];
        let n = client.read(&mut buf).expect("client should read greeting");
        assert_eq!(&buf[..n], b"220 Service ready\r\n");

        drop(client);
        handle
            .join()
            .expect("session thread should join")
            .expect("session should complete cleanly");
    }

    // GIVEN a running broker WHEN a client runs a full HELO/MAIL/RCPT/DATA/QUIT
    // exchange THEN every reply has the expected status code
    #[test]
    fn test_session_full_smtp_transaction() {
        let (addr, handle) = spawn_one_shot_broker();

        let stream = TcpStream::connect(addr).expect("client should connect");
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("read timeout should set");
        let mut writer = stream.try_clone().expect("clone for writer");
        let mut reader = BufReader::new(stream);

        let mut read_line = || {
            let mut line = String::new();
            reader.read_line(&mut line).expect("read reply");
            line
        };
        let send = |w: &mut TcpStream, line: &str| {
            w.write_all(line.as_bytes()).expect("write command");
        };

        assert!(read_line().starts_with("220 "));

        send(&mut writer, "HELO client.example\r\n");
        assert!(read_line().starts_with("250 "));

        send(&mut writer, "MAIL FROM:<a@b>\r\n");
        assert!(read_line().starts_with("250 "));

        send(&mut writer, "RCPT TO:<c@d>\r\n");
        assert!(read_line().starts_with("250 "));

        send(&mut writer, "DATA\r\n");
        assert!(read_line().starts_with("354 "));

        send(&mut writer, "Subject: hi\r\n");
        send(&mut writer, "\r\n");
        send(&mut writer, "body line\r\n");
        send(&mut writer, ".\r\n");
        assert!(read_line().starts_with("250 "));

        send(&mut writer, "QUIT\r\n");
        assert!(read_line().starts_with("221 "));

        drop(writer);
        handle
            .join()
            .expect("session thread should join")
            .expect("session should complete cleanly");
    }

    // GIVEN a broker accepting in a background thread WHEN two clients connect concurrently
    // THEN both receive their greeting without one having to finish first.
    // This catches the regression of a single-threaded accept loop, where the
    // second client's greeting would block until the first client's session ends.
    #[test]
    fn test_broker_handles_concurrent_sessions() {
        let broker = Broker::new("127.0.0.1:0").expect("broker should bind");
        let addr = broker.local_addr().expect("local addr");
        thread::spawn(move || {
            let _ = broker.accept();
        });

        let mut c1 = TcpStream::connect(addr).expect("c1 connect");
        let mut c2 = TcpStream::connect(addr).expect("c2 connect");
        c1.set_read_timeout(Some(Duration::from_secs(2)))
            .expect("c1 read timeout");
        c2.set_read_timeout(Some(Duration::from_secs(2)))
            .expect("c2 read timeout");

        // c1 stays silent on purpose. With a single-threaded broker this would
        // block c2 from ever receiving its greeting and the read would time
        // out. With per-session threads, c2's greeting arrives immediately.
        let mut b2 = [0u8; 64];
        let n2 = c2.read(&mut b2).expect("c2 should receive greeting");
        assert_eq!(&b2[..n2], b"220 Service ready\r\n");

        let mut b1 = [0u8; 64];
        let n1 = c1.read(&mut b1).expect("c1 should receive greeting");
        assert_eq!(&b1[..n1], b"220 Service ready\r\n");
    }

    // GIVEN a broker accepting in a background thread WHEN a session errors
    // THEN subsequent sessions still succeed (the broker stays alive).
    #[test]
    fn test_broker_survives_session_errors() {
        let broker = Broker::new("127.0.0.1:0").expect("broker should bind");
        let addr = broker.local_addr().expect("local addr");
        thread::spawn(move || {
            let _ = broker.accept();
        });

        // First client connects and immediately drops, simulating an aborted
        // session (its handle() will hit EOF / connection-reset).
        {
            let bad = TcpStream::connect(addr).expect("bad client connect");
            drop(bad);
        }

        // The broker must still be accepting.
        let mut good = TcpStream::connect(addr).expect("good client connect");
        good.set_read_timeout(Some(Duration::from_secs(2)))
            .expect("set read timeout");
        let mut buf = [0u8; 64];
        let n = good
            .read(&mut buf)
            .expect("good client should receive greeting");
        assert_eq!(&buf[..n], b"220 Service ready\r\n");
    }

    // GIVEN a connected client WHEN it sends an unknown command
    // THEN the session replies with 500 and stays open
    #[test]
    fn test_session_unknown_command_replies_500() {
        let (addr, handle) = spawn_one_shot_broker();

        let stream = TcpStream::connect(addr).expect("client should connect");
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("read timeout should set");
        let mut writer = stream.try_clone().expect("clone for writer");
        let mut reader = BufReader::new(stream);

        let mut greeting = String::new();
        reader.read_line(&mut greeting).expect("read greeting");
        assert!(greeting.starts_with("220 "));

        writer
            .write_all(b"FOOBAR\r\n")
            .expect("write unknown command");
        let mut reply = String::new();
        reader.read_line(&mut reply).expect("read reply");
        assert!(reply.starts_with("500 "), "got reply: {}", reply);

        writer.write_all(b"QUIT\r\n").expect("write quit");
        let mut bye = String::new();
        reader.read_line(&mut bye).expect("read bye");
        assert!(bye.starts_with("221 "));

        drop(writer);
        handle
            .join()
            .expect("session thread should join")
            .expect("session should complete cleanly");
    }
}
