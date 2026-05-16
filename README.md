# smtp-rs

Implementation of the SMTP protocol in Rust. This crate provides a library (`smtp-rs`) with the protocol building blocks, plus two binaries built on top of it: minimal client and server.

Aiming for compliance with [RFC 821](https://www.rfc-editor.org/pdfrfc/rfc821.txt.pdf).

## Building

```sh
cargo build
```

## Running the server

```sh
cargo run --bin server
```

The server will block, accepting connections until you stop it with `Ctrl+C`. Each accepted mail is printed to stdout.

## Running the client

With the server running in another terminal:

```sh
cargo run --bin client
```

The client connects to `127.0.0.1:2525`, performs a full SMTP transaction (`HELO`/`MAIL`/`RCPT`/`DATA`/`QUIT`), and prints the server replies.

## Roadmap

- [x] SMTP command parsing and reply formatting
- [x] SMTP session state machine
- [x] TCP session broker and server
- [x] Transport abstraction with in-memory and null transports
- [ ] Persist accepted mail to disk (spool/queue)
- [ ] Add multi-recipient delivery hooks
- [ ] Improve error reporting and logging
- [ ] Add TLS and AUTH extensions

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

You should see SMTP replies on the client and the accepted mail printed by the server.

You can also connect interactively:

```sh
nc -C 127.0.0.1 2525
```
