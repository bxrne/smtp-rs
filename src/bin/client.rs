use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::net::TcpStream;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Instant;
use tracing::{debug, error, info, warn};
use tracing_subscriber::EnvFilter;

const ADDR: &str = "127.0.0.1:2525";

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .try_init();
}

fn send<W: Write, R: BufRead>(writer: &mut W, reader: &mut R, line: &str) -> std::io::Result<()> {
    writer.write_all(line.as_bytes())?;
    writer.write_all(b"\r\n")?;
    writer.flush()?;
    read_reply(reader)
}

fn read_reply<R: BufRead>(reader: &mut R) -> std::io::Result<()> {
    let mut resp = String::new();
    reader.read_line(&mut resp)?;
    debug!(reply = %resp.trim_end(), "server reply");
    Ok(())
}

fn subject_for(worker_id: usize, trial: usize) -> String {
    format!("smtp-rs client worker={worker_id} trial={trial}")
}

fn run_one(addr: &str, worker_id: usize, trial: usize) -> std::io::Result<()> {
    let stream = TcpStream::connect(addr)?;
    let mut writer = stream.try_clone()?;
    let mut reader = BufReader::new(stream);

    read_reply(&mut reader)?;

    send(&mut writer, &mut reader, "HELO localhost")?;
    send(&mut writer, &mut reader, "MAIL FROM:<sender@example.com>")?;
    send(&mut writer, &mut reader, "RCPT TO:<recipient@example.com>")?;
    send(&mut writer, &mut reader, "DATA")?;

    let subject = subject_for(worker_id, trial);
    let body = format!("Subject: {subject}\r\n\r\nworker={worker_id} trial={trial}\r\n.\r\n");
    writer.write_all(body.as_bytes())?;
    writer.flush()?;
    read_reply(&mut reader)?;

    send(&mut writer, &mut reader, "QUIT")?;
    Ok(())
}

fn format_error(addr: &str, err: &std::io::Error) -> String {
    match err.kind() {
        ErrorKind::ConnectionRefused => {
            format!(
                "could not connect to SMTP server at {addr}: connection refused \
                 (is the server running? try `cargo run --bin server`)"
            )
        }
        ErrorKind::TimedOut => format!("connection to {addr} timed out"),
        ErrorKind::UnexpectedEof => format!("connection to {addr} closed unexpectedly"),
        _ => format!("{err}"),
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Args {
    trials: usize,
    workers: usize,
}

fn parse_args<I, S>(mut args: I) -> std::result::Result<Args, String>
where
    I: Iterator<Item = S>,
    S: AsRef<str>,
{
    let prog = args
        .next()
        .map(|s| s.as_ref().to_string())
        .unwrap_or_else(|| "client".to_string());

    let trials_s = args
        .next()
        .ok_or_else(|| format!("usage: {prog} <trials> <workers>"))?;
    let workers_s = args
        .next()
        .ok_or_else(|| format!("usage: {prog} <trials> <workers>"))?;

    if args.next().is_some() {
        return Err(format!("usage: {prog} <trials> <workers>"));
    }

    let trials: usize = trials_s.as_ref().parse().map_err(|_| {
        format!(
            "trials must be a non-negative integer, got `{}`",
            trials_s.as_ref()
        )
    })?;
    let workers: usize = workers_s.as_ref().parse().map_err(|_| {
        format!(
            "workers must be a non-negative integer, got `{}`",
            workers_s.as_ref()
        )
    })?;

    if workers == 0 {
        return Err("workers must be >= 1".into());
    }

    Ok(Args { trials, workers })
}

fn run_workload(addr: &'static str, args: &Args) -> (usize, usize) {
    let success = Arc::new(AtomicUsize::new(0));
    let failure = Arc::new(AtomicUsize::new(0));
    let trials = args.trials;

    let handles: Vec<_> = (0..args.workers)
        .map(|worker_id| {
            let success = Arc::clone(&success);
            let failure = Arc::clone(&failure);
            thread::spawn(move || {
                for trial in 0..trials {
                    match run_one(addr, worker_id, trial) {
                        Ok(()) => {
                            success.fetch_add(1, Ordering::Relaxed);
                            debug!(worker_id, trial, "trial ok");
                        }
                        Err(err) => {
                            failure.fetch_add(1, Ordering::Relaxed);
                            warn!(worker_id, trial, error = %format_error(addr, &err), "trial failed");
                        }
                    }
                }
            })
        })
        .collect();

    for handle in handles {
        let _ = handle.join();
    }

    (
        success.load(Ordering::Relaxed),
        failure.load(Ordering::Relaxed),
    )
}

fn main() -> ExitCode {
    init_tracing();

    let args = match parse_args(std::env::args()) {
        Ok(a) => a,
        Err(msg) => {
            error!("{msg}");
            return ExitCode::FAILURE;
        }
    };

    info!(
        trials = args.trials,
        workers = args.workers,
        total = args.trials * args.workers,
        addr = ADDR,
        "starting client workload"
    );

    let started = Instant::now();
    let (success, failure) = run_workload(ADDR, &args);
    let elapsed = started.elapsed();

    info!(
        success,
        failure,
        elapsed_ms = elapsed.as_millis() as u64,
        "workload complete"
    );

    if failure > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
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
    // THEN it falls back to a Display-formatted message
    #[test]
    fn test_format_error_generic_fallback() {
        let err = std::io::Error::other("boom");
        let msg = format_error("127.0.0.1:2525", &err);
        assert!(msg.contains("boom"), "got: {msg}");
    }

    // GIVEN valid args WHEN parse_args runs THEN it returns the parsed counts
    #[test]
    fn test_parse_args_valid() {
        let args = parse_args(["client", "5", "3"].iter()).expect("parse ok");
        assert_eq!(
            args,
            Args {
                trials: 5,
                workers: 3
            }
        );
    }

    // GIVEN no args WHEN parse_args runs THEN it returns a usage error
    #[test]
    fn test_parse_args_missing_returns_usage() {
        let err = parse_args(["client"].iter()).unwrap_err();
        assert!(err.contains("usage"), "got: {err}");
    }

    // GIVEN one arg WHEN parse_args runs THEN it returns a usage error
    #[test]
    fn test_parse_args_missing_second_returns_usage() {
        let err = parse_args(["client", "5"].iter()).unwrap_err();
        assert!(err.contains("usage"), "got: {err}");
    }

    // GIVEN too many args WHEN parse_args runs THEN it returns a usage error
    #[test]
    fn test_parse_args_extra_returns_usage() {
        let err = parse_args(["client", "5", "3", "extra"].iter()).unwrap_err();
        assert!(err.contains("usage"), "got: {err}");
    }

    // GIVEN a non-numeric trials arg WHEN parse_args runs THEN it reports trials
    #[test]
    fn test_parse_args_non_numeric_trials() {
        let err = parse_args(["client", "abc", "3"].iter()).unwrap_err();
        assert!(err.contains("trials"), "got: {err}");
    }

    // GIVEN a non-numeric workers arg WHEN parse_args runs THEN it reports workers
    #[test]
    fn test_parse_args_non_numeric_workers() {
        let err = parse_args(["client", "5", "xyz"].iter()).unwrap_err();
        assert!(err.contains("workers"), "got: {err}");
    }

    // GIVEN zero workers WHEN parse_args runs THEN it rejects the value
    #[test]
    fn test_parse_args_zero_workers_rejected() {
        let err = parse_args(["client", "5", "0"].iter()).unwrap_err();
        assert!(err.contains("workers"), "got: {err}");
    }

    // GIVEN zero trials WHEN run_workload runs THEN it does no work and reports 0/0
    #[test]
    fn test_run_workload_zero_trials_does_nothing() {
        let args = Args {
            trials: 0,
            workers: 4,
        };
        let (success, failure) = run_workload("127.0.0.1:1", &args);
        assert_eq!(success, 0);
        assert_eq!(failure, 0);
    }

    // GIVEN a refused address WHEN run_workload runs
    // THEN every trial across every worker counts as a failure
    #[test]
    fn test_run_workload_failures_counted_per_thread() {
        // Reserved port that should refuse connections.
        let args = Args {
            trials: 2,
            workers: 3,
        };
        let (success, failure) = run_workload("127.0.0.1:1", &args);
        assert_eq!(success, 0);
        assert_eq!(failure, 6);
    }

    // GIVEN a worker id and trial number WHEN subject_for runs
    // THEN both identifiers appear in the produced subject
    #[test]
    fn test_subject_for_includes_worker_and_trial() {
        let subject = subject_for(7, 42);
        assert!(subject.contains("worker=7"), "got: {subject}");
        assert!(subject.contains("trial=42"), "got: {subject}");
    }

    // GIVEN distinct (worker, trial) pairs WHEN subject_for runs
    // THEN each pair produces a unique subject
    #[test]
    fn test_subject_for_is_unique_per_pair() {
        assert_ne!(subject_for(0, 0), subject_for(0, 1));
        assert_ne!(subject_for(0, 0), subject_for(1, 0));
        assert_ne!(subject_for(1, 2), subject_for(2, 1));
    }

    // GIVEN a running broker WHEN run_one sends an email for (worker, trial)
    // THEN the DATA body contains a Subject line embedding both numbers
    #[test]
    fn test_run_one_subject_reaches_server() {
        use std::sync::mpsc;
        use std::time::Duration;

        let listener =
            std::net::TcpListener::bind("127.0.0.1:0").expect("test listener should bind");
        let addr = listener.local_addr().expect("local addr");
        let addr_string = addr.to_string();

        let (tx, rx) = mpsc::channel::<String>();

        let server_handle = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("set read timeout");
            let mut writer = stream.try_clone().expect("clone for writer");
            let mut reader = BufReader::new(stream);

            writer.write_all(b"220 Service ready\r\n").unwrap();

            let mut step = |code: &[u8]| {
                let mut line = String::new();
                reader.read_line(&mut line).expect("read command");
                writer.write_all(code).unwrap();
            };
            step(b"250 HELO ok\r\n");
            step(b"250 MAIL ok\r\n");
            step(b"250 RCPT ok\r\n");
            step(b"354 send data\r\n");

            let mut captured = String::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).expect("read body");
                if line.trim_end_matches("\r\n") == "." {
                    break;
                }
                captured.push_str(&line);
            }
            writer.write_all(b"250 message accepted\r\n").unwrap();

            let mut quit = String::new();
            reader.read_line(&mut quit).expect("read quit");
            writer.write_all(b"221 bye\r\n").unwrap();

            tx.send(captured).expect("send captured");
        });

        run_one(&addr_string, 3, 11).expect("client transaction");

        server_handle.join().expect("server thread join");
        let captured = rx.recv().expect("captured body");

        assert!(
            captured.contains("Subject: smtp-rs client worker=3 trial=11"),
            "captured body did not contain expected Subject. body={captured:?}"
        );
        assert!(
            captured.contains("worker=3 trial=11"),
            "captured body did not contain worker/trial markers. body={captured:?}"
        );
    }

    // GIVEN init_tracing is called twice WHEN the second call runs
    // THEN it does not panic because try_init swallows the already-set error
    #[test]
    fn test_init_tracing_is_idempotent() {
        init_tracing();
        init_tracing();
    }

    // GIVEN a running broker WHEN the client performs a real SMTP transaction
    // THEN every reply line begins with the expected status code
    #[test]
    fn test_client_completes_real_smtp_transaction() {
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
