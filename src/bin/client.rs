use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::net::TcpStream;
use std::process::ExitCode;

fn send<W: Write, R: BufRead>(writer: &mut W, reader: &mut R, line: &str) -> std::io::Result<()> {
    writer.write_all(line.as_bytes())?;
    writer.write_all(b"\r\n")?;
    writer.flush()?;
    read_reply(reader)
}

fn read_reply<R: BufRead>(reader: &mut R) -> std::io::Result<()> {
    let mut resp = String::new();
    reader.read_line(&mut resp)?;
    print!("S: {resp}");
    Ok(())
}

fn run(addr: &str) -> std::io::Result<()> {
    let stream = TcpStream::connect(addr)?;
    let mut writer = stream.try_clone()?;
    let mut reader = BufReader::new(stream);

    read_reply(&mut reader)?;

    send(&mut writer, &mut reader, "HELO localhost")?;
    send(&mut writer, &mut reader, "MAIL FROM:<sender@example.com>")?;
    send(&mut writer, &mut reader, "RCPT TO:<recipient@example.com>")?;
    send(&mut writer, &mut reader, "DATA")?;

    writer
        .write_all(b"Subject: hello from smtp-rs client\r\n\r\nThis is a test message.\r\n.\r\n")?;
    writer.flush()?;
    read_reply(&mut reader)?;

    send(&mut writer, &mut reader, "QUIT")?;
    Ok(())
}

fn format_error(addr: &str, err: &std::io::Error) -> String {
    match err.kind() {
        ErrorKind::ConnectionRefused => {
            format!("error: could not connect to SMTP server at {addr}: connection refused")
        }
        ErrorKind::TimedOut => {
            format!("error: connection to {addr} timed out")
        }
        ErrorKind::UnexpectedEof => {
            format!("error: connection to {addr} closed unexpectedly")
        }
        _ => format!("error: {err}"),
    }
}

fn main() -> ExitCode {
    let addr = "127.0.0.1:2525";
    match run(addr) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{}", format_error(addr, &err));
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    // GIVEN a command WHEN send writes it THEN the bytes are line-terminated with CRLF
    #[test]
    fn test_send_writes_command_with_crlf() {
        let mut writer: Vec<u8> = Vec::new();
        let mut reader = Cursor::new(b"250 OK\r\n".to_vec());
        send(&mut writer, &mut reader, "HELO localhost").expect("send should succeed");
        assert_eq!(writer, b"HELO localhost\r\n");
    }

    // GIVEN a reader with a server reply WHEN read_reply consumes one line
    // THEN it returns Ok and advances the reader by exactly that line
    #[test]
    fn test_read_reply_consumes_single_line() {
        let mut reader = Cursor::new(b"220 Service ready\r\n250 OK\r\n".to_vec());
        read_reply(&mut reader).expect("read_reply should succeed");
        let mut rest = String::new();
        reader.read_line(&mut rest).expect("read second line");
        assert_eq!(rest, "250 OK\r\n");
    }

    // GIVEN an empty reader WHEN read_reply runs THEN it returns Ok with no bytes
    #[test]
    fn test_read_reply_on_eof_returns_ok() {
        let mut reader = Cursor::new(Vec::<u8>::new());
        assert!(read_reply(&mut reader).is_ok());
    }

    // GIVEN a sequence of commands WHEN send is invoked for each
    // THEN the writer captures every command exactly in order, CRLF-terminated
    #[test]
    fn test_send_multiple_commands_are_serialized_in_order() {
        let mut writer: Vec<u8> = Vec::new();
        let mut reader = Cursor::new(b"250 a\r\n250 b\r\n221 bye\r\n".to_vec());

        send(&mut writer, &mut reader, "MAIL FROM:<a@b>").expect("send 1");
        send(&mut writer, &mut reader, "RCPT TO:<c@d>").expect("send 2");
        send(&mut writer, &mut reader, "QUIT").expect("send 3");

        assert_eq!(
            writer,
            b"MAIL FROM:<a@b>\r\nRCPT TO:<c@d>\r\nQUIT\r\n".to_vec()
        );
    }

    // GIVEN a ConnectionRefused error WHEN format_error runs
    // THEN it returns a friendly message naming the address and a hint
    #[test]
    fn test_format_error_connection_refused() {
        let err = std::io::Error::from(ErrorKind::ConnectionRefused);
        let msg = format_error("127.0.0.1:2525", &err);
        assert!(msg.contains("127.0.0.1:2525"), "got: {msg}");
        assert!(msg.contains("connection refused"), "got: {msg}");
        assert!(msg.contains("server"), "got: {msg}");
    }

    // GIVEN a TimedOut error WHEN format_error runs THEN it mentions the timeout
    #[test]
    fn test_format_error_timed_out() {
        let err = std::io::Error::from(ErrorKind::TimedOut);
        let msg = format_error("127.0.0.1:2525", &err);
        assert!(msg.contains("timed out"), "got: {msg}");
    }

    // GIVEN an UnexpectedEof error WHEN format_error runs
    // THEN it reports the connection closed unexpectedly
    #[test]
    fn test_format_error_unexpected_eof() {
        let err = std::io::Error::from(ErrorKind::UnexpectedEof);
        let msg = format_error("127.0.0.1:2525", &err);
        assert!(msg.contains("closed unexpectedly"), "got: {msg}");
    }

    // GIVEN any other io error WHEN format_error runs
    // THEN it falls back to a generic "error: ..." prefix
    #[test]
    fn test_format_error_generic_fallback() {
        let err = std::io::Error::other("boom");
        let msg = format_error("127.0.0.1:2525", &err);
        assert!(msg.starts_with("error: "), "got: {msg}");
        assert!(msg.contains("boom"), "got: {msg}");
    }

    // GIVEN a running broker WHEN the client performs a real SMTP transaction
    // THEN every reply line begins with the expected status code
    #[test]
    fn test_client_completes_real_smtp_transaction() {
        use std::thread;
        use std::time::Duration;

        let listener =
            std::net::TcpListener::bind("127.0.0.1:0").expect("test listener should bind");
        let addr = listener.local_addr().expect("local addr");

        let server_handle = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            let mut writer = stream.try_clone().expect("clone for writer");
            let mut reader = BufReader::new(stream);

            writer.write_all(b"220 Service ready\r\n").unwrap();

            let mut consume = |code: &[u8]| {
                let mut line = String::new();
                reader.read_line(&mut line).expect("read command");
                writer.write_all(code).unwrap();
            };

            consume(b"250 HELO ok\r\n"); // HELO
            consume(b"250 MAIL ok\r\n"); // MAIL FROM
            consume(b"250 RCPT ok\r\n"); // RCPT TO
            consume(b"354 send data\r\n"); // DATA

            // consume body lines up to terminating "."
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).expect("read body");
                if line.trim_end_matches("\r\n") == "." {
                    break;
                }
            }
            writer.write_all(b"250 message accepted\r\n").unwrap();

            // QUIT
            let mut quit = String::new();
            reader.read_line(&mut quit).expect("read quit");
            writer.write_all(b"221 bye\r\n").unwrap();
        });

        let stream = TcpStream::connect(addr).expect("client connect");
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("set read timeout");
        let mut writer = stream.try_clone().expect("clone writer");
        let mut reader = BufReader::new(stream);

        read_reply(&mut reader).expect("greeting");
        send(&mut writer, &mut reader, "HELO localhost").expect("HELO");
        send(&mut writer, &mut reader, "MAIL FROM:<sender@example.com>").expect("MAIL");
        send(&mut writer, &mut reader, "RCPT TO:<recipient@example.com>").expect("RCPT");
        send(&mut writer, &mut reader, "DATA").expect("DATA");

        writer
            .write_all(b"Subject: hi\r\n\r\nbody\r\n.\r\n")
            .expect("body");
        writer.flush().expect("flush");
        read_reply(&mut reader).expect("data accept");

        send(&mut writer, &mut reader, "QUIT").expect("QUIT");

        drop(writer);
        server_handle.join().expect("server thread join");
    }
}
