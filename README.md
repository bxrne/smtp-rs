# smtp-rs

Implementation of the SMTP protocol in Rust. This library provides a simple and efficient way to send emails using the SMTP protocol.

Aiming for compliance with [RFC 821](https://www.rfc-editor.org/pdfrfc/rfc821.txt.pdf).

## Building

```sh
cargo build
```

## Running the example server

```sh
cargo run --bin example
```

The server will block, accepting connections until you stop it with `Ctrl+C`.

## Running the transport demo (prints accepted mail)

```sh
cargo run --bin nc_transport
```

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

In one terminal, start the transport demo server:

```sh
cargo run --bin nc_transport
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
