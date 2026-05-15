# smtp-rs

Implementation of the SMTP protocol in Rust. This library provides a simple and efficient way to send emails using the SMTP protocol.

Aiming for compliance with [RFC 821](https://www.rfc-editor.org/pdfrfc/rfc821.txt.pdf).

## Building

```sh
cargo build
```

## Running the example server

```sh
cargo run --bin smtp-example
```

The server will block, accepting connections until you stop it with `Ctrl+C`.

## Testing

Run the full test suite:

```sh
cargo test
```

### Integration testing with `netcat`

In one terminal, start the server:

```sh
cargo run --bin smtp-example
```

In another terminal, connect with `nc` (netcat):

```sh
nc 127.0.0.1 2525
```

You should immediately see the greeting:

```
220 Service ready
```

Alternatively, send an empty payload and read one reply in a single command:

```sh
printf '' | nc -w 2 127.0.0.1 2525
```

You can also use `telnet` if `nc` is not available:

```sh
telnet 127.0.0.1 2525
```
