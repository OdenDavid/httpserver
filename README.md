# rustserver

Working name: rustserver.
Goal: Build a working HTTP server from raw TCP connections using Rust's standard library, then evolve it into a concurrent server and eventually compare it with a production implementation using Tokio and Axum.

**Status: V1 — a TCP server, one connection at a time.** It speaks TCP, not HTTP yet, and it
is an educational implementation rather than something to expose to the internet.
Companion notes: [`docs/V1.md`](docs/V1.md).

```console
$ cargo run
rustserver: listening on 127.0.0.1:7878
rustserver: try `printf 'hello\n' | nc 127.0.0.1:7878`

# in another terminal
$ printf 'hello\n' | nc 127.0.0.1 7878
rustserver v1: read your bytes, closing now
```

## Build and run

Rust 1.85 or newer, and no dependencies at all: `std` only, on purpose.

```sh
cargo run                  # listens on 127.0.0.1:7878
cargo run -- 127.0.0.1:9000
cargo build --release      # -> target/release/rustserver
```

```text
rustserver [address]

  address        host:port to listen on          (default 127.0.0.1:7878)
  -h, --help     this message
```

Exit codes: `0` clean, `1` the socket could not be used, `2` the command line was wrong.

## What V1 does

- Binds a TCP port and prints the address it actually got.
- Accepts one connection at a time, reads **once** into a 1024-byte buffer, prints what
  arrived, replies in plain bytes, and closes.
- Reports failures instead of swallowing them: a bad address is fatal (exit 1), a broken
  connection is printed and the next client is still served.

What it deliberately does not do: HTTP, concurrency, timeouts, TLS, and everything else a
real server needs. A browser that visits it is told

```console
curl: (1) Received HTTP/0.9 when not allowed
```

which is exactly the next version's problem.

## Tests

```sh
cargo test                    # 9: 4 unit + 5 end-to-end
cargo test --test server      # end-to-end only: they run the real binary
cargo clippy --all-targets    # clean
```

The unit tests call the connection handler against real sockets on a port the kernel picks;
the end-to-end tests start the real binary, find its port in its own greeting, talk to it
over a socket, and check its transcript.

## Versions

| version | what it added | notes |
| --- | --- | --- |
| v1 | the socket: bind, accept, read once, print, answer, close | [`docs/V1.md`](docs/V1.md) |
| v2 | HTTP: parse a request, answer with a real response | planned |

The rule carried over from the first project: each version works, has tests, and is written
down before the next one starts.

