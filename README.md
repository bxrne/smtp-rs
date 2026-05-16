# smtp-rs

Implementation of the SMTP protocol in Rust. This crate provides a library (`smtp_rs`) with the protocol building blocks, plus two binaries built on top of it:

- `server` — a concurrent SMTP server that listens on `127.0.0.1:2525` and prints accepted mail via a custom `Transport`. Each connection is handled on its own thread.
- `client` — a multi-threaded SMTP client used as a load generator. Takes `<trials> <workers>` positional arguments and spawns `workers` threads, each sending `trials` emails.

Both binaries emit structured logs via [`tracing`](https://docs.rs/tracing). Log level is controlled by `RUST_LOG` (defaults to `info`).

Aiming for compliance with [RFC 821](https://www.rfc-editor.org/pdfrfc/rfc821.txt.pdf).

## Building

```sh
cargo build
```

## Running the server

```sh
cargo run --bin server
```

The server binds `127.0.0.1:2525` and accepts connections until you stop it with `Ctrl+C`. Each accepted mail is logged at `info` level. Per-session errors are logged at `warn` and do not stop the broker.

Tweak verbosity with `RUST_LOG`:

```sh
RUST_LOG=debug cargo run --bin server
```

## Running the client

The client takes two positional arguments:

```
client <trials> <workers>
```

- `trials` — number of emails each worker thread sends (≥ 0).
- `workers` — number of concurrent worker threads (≥ 1).

Each email's subject and body embed the worker id and trial number (e.g. `Subject: smtp-rs client worker=3 trial=11`) so individual messages are identifiable on the server side.

Example — 10 worker threads, each sending 20 emails (200 total):

```sh
cargo run --bin client 20 10
```

The client prints a startup line, per-trial warnings on failure, and a final summary:

```
INFO starting client workload trials=20 workers=10 total=200 addr="127.0.0.1:2525"
INFO workload complete success=200 failure=0 elapsed_ms=3323
```

Exit code is `0` if every trial succeeded, `1` otherwise. Connection-level errors get friendly messages (e.g. "connection refused — is the server running?").

## Todos 

- [x] SMTP command parsing and reply formatting
- [x] SMTP session state machine
- [x] TCP session broker and server
- [x] Transport abstraction with in-memory and null transports
- [x] Per-session threading so the server handles concurrent clients
- [x] Structured logging in both binaries via `tracing` / `tracing-subscriber`
- [ ] Persist accepted mail to disk (spool/queue)
- [ ] Add multi-recipient delivery hooks
- [ ] Add TLS and AUTH extensions
- [ ] Bound the number of in-flight sessions (thread pool / connection limit)

## Testing

> TODO: Find a way to run a RFC 821 compliance test suite against the library.

Run the full test suite:

```sh
cargo test
```

### Integration testing with `netcat`

In one terminal, start the server:

```sh
cargo run --bin server
```

In another terminal, send a full SMTP transaction:

```sh
printf "HELO client\r\nMAIL FROM:<a@b>\r\nRCPT TO:<c@d>\r\nDATA\r\nSubject: hi\r\n\r\nhello\r\n.\r\nQUIT\r\n" | nc 127.0.0.1 2525
```

You should see SMTP replies on the client and the accepted mail logged by the server.

You can also connect interactively:

```sh
nc -C 127.0.0.1 2525
```

### Load testing with the client binary

With the server running:

```sh
cargo run --bin client 20 10   # 10 workers × 20 trials = 200 emails
```

The client's `workload complete` line reports success/failure counts and elapsed time.
