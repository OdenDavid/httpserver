//! A TCP server built on the standard library, one connection at a time.
//!
//! Version 1 of `rustserver`. It listens on `127.0.0.1:7878` (or wherever the command
//! line points it), accepts a connection, reads once into a fixed buffer, prints what
//! arrived, writes a few bytes back, and closes. No HTTP parsing, no threads, no
//! dependencies: the subject of this version is the socket itself.
//!
//! ```text
//! $ cargo run
//! rustserver: listening on 127.0.0.1:7878
//! rustserver: try `printf 'hello\n' | nc 127.0.0.1:7878`
//! rustserver: 6 bytes from 127.0.0.1:52359
//! rustserver: | hello
//! rustserver: sent 44 bytes, closing
//! ```

use std::fmt;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};

/// Where V1 listens
const DEFAULT_ADDR: &str = "127.0.0.1:7878";

/// How many bytes for one `read`
const READ_BUFFER: usize = 1024;

/// What every client is sent. Not HTTP: this version is about TCP.
const RESPONSE: &[u8] = b"rustserver v1: read your bytes, closing now\n";

const USAGE: &str = "\
rustserver [address]

  address        host:port to listen on          (default 127.0.0.1:7878)
  -h, --help     this message

Version 1 serves one connection at a time: it reads once, prints what arrived, answers
in plain bytes and closes. It speaks TCP, not HTTP.
";

type Result<T> = std::result::Result<T, Error>;

/// Everything error for V1
#[derive(Debug)]
enum Error {
    Usage,
    /// The listening socket could not be created: bad address, or port already taken.
    Listen {
        addr: String,
        source: io::Error,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Usage => write!(f, "the command line was wrong"),
            Error::Listen { addr, source } => write!(f, "cannot listen on {addr}: {source}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Usage => None,
            Error::Listen { source, .. } => Some(source),
        }
    }
}

impl Error {
    fn is_usage(&self) -> bool {
        matches!(self, Error::Usage)
    }

    fn exit_code(&self) -> i32 {
        match self {
            Error::Usage => 2,
            Error::Listen { .. } => 1,
        }
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("rustserver: {error}");
        if error.is_usage() {
            eprintln!("\n{USAGE}");
        }
        std::process::exit(error.exit_code());
    }
}

fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        println!("{USAGE}");
        return Ok(());
    }

    let addr = match args.as_slice() {
        [] => DEFAULT_ADDR.to_string(),
        [addr] => addr.clone(),
        _ => return Err(Error::Usage),
    };

    let listener = TcpListener::bind(&addr).map_err(|source| Error::Listen {
        addr: addr.clone(),
        source,
    })?;

    let local = listener.local_addr().map_err(|source| Error::Listen {
        addr: addr.clone(),
        source,
    })?;

    println!("rustserver: listening on {local}");
    println!("rustserver: try `printf 'hello\\n' | nc {local}`");

    serve(listener);

    Ok(())
}

/// Accepts connections, one at a time, for as long as the program lives
fn serve(listener: TcpListener) {
    let mut out = io::stdout();

    // `incoming()` is an iterator of `io::Result<TcpStream>`, one item per connection:
    for incoming in listener.incoming() {
        // A failure here belongs to that client, not to the server: report it and keep
        // listening do not taking the whole program down.
        let stream = match incoming {
            Ok(stream) => stream,
            Err(source) => {
                eprintln!("rustserver: could not accept a connection: {source}");
                continue;
            }
        };

        // Who is on the other end, as the kernel sees them.
        let peer = match stream.peer_addr() {
            Ok(peer) => peer,
            Err(source) => {
                eprintln!("rustserver: accepted a connection with no address: {source}");
                continue;
            }
        };

        if let Err(source) = handle_connection(stream, peer, &mut out) {
            eprintln!("rustserver: {peer}: {source}");
        }
    }
}

/// One connection, start to finish: read once, print what arrived, answer, report the count
fn handle_connection(
    mut stream: TcpStream,
    peer: SocketAddr,
    log: &mut impl Write,
) -> io::Result<usize> {
    let mut buffer = [0u8; READ_BUFFER];

    let bytes_read = stream.read(&mut buffer)?;

    let received = &buffer[..bytes_read];

    if bytes_read == 0 {
        writeln!(
            log,
            "rustserver: 0 bytes from {peer} (closed before sending anything)"
        )?;
        return Ok(0);
    }

    writeln!(log, "rustserver: {bytes_read} bytes from {peer}")?;

    for line in String::from_utf8_lossy(received).lines() {
        writeln!(log, "rustserver: | {line}")?;
    }

    stream.write_all(RESPONSE)?;
    writeln!(log, "rustserver: sent {} bytes, closing", RESPONSE.len())?;

    Ok(bytes_read)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Shutdown;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    /// The handler under a real socket, with the smallest possible `serve`: a listener on
    /// a port the kernel picks, one connection accepted in a background thread and the
    /// log handed back through a channel so the test can read it.
    fn serve_one() -> (SocketAddr, mpsc::Receiver<(String, io::Result<usize>)>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind a test listener");
        let addr = listener
            .local_addr()
            .expect("a bound listener has an address");
        let (tx, rx) = mpsc::channel();

        thread::spawn(move || {
            let (stream, peer) = listener.accept().expect("accept one connection");
            let mut log = Vec::new();
            let result = handle_connection(stream, peer, &mut log);

            let _ = tx.send((String::from_utf8_lossy(&log).into_owned(), result));
        });

        (addr, rx)
    }

    /// A client that fails rather than waits forever if the server goes quiet.
    fn client(addr: SocketAddr) -> TcpStream {
        let stream = TcpStream::connect(addr).expect("connect to the test server");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("set a read timeout");
        stream
    }

    #[test]
    fn the_server_reads_what_the_client_sends_and_answers_it() {
        let (addr, outcome) = serve_one();
        let request = b"hello from the test\n";

        let mut client = client(addr);
        client.write_all(request).expect("send the request");

        // Reads until the server closes, which is the fifth requirement, answer then
        // close. Without the read timeout above, a server that never closed would hang
        // this test instead of failing it.
        let mut response = Vec::new();
        client
            .read_to_end(&mut response)
            .expect("read the response");
        assert_eq!(response, RESPONSE);

        let (log, result) = outcome.recv().expect("the handler should have finished");
        assert_eq!(result.expect("no error"), request.len());
        assert!(
            log.contains(&format!("{} bytes from 127.0.0.1:", request.len())),
            "the log should name the byte count: {log:?}"
        );
        assert!(
            log.contains("| hello from the test"),
            "the log should print what arrived: {log:?}"
        );
    }

    #[test]
    fn a_client_that_sends_nothing_is_end_of_input_not_an_empty_message() {
        let (addr, outcome) = serve_one();

        // Half-close: the client can still read, but the server is told there is no more
        // data coming. `read` then returns 0, its only way of saying "end of input".
        let mut client = client(addr);
        client.shutdown(Shutdown::Write).expect("half-close");

        let mut response = Vec::new();
        client
            .read_to_end(&mut response)
            .expect("read the response");
        assert!(
            response.is_empty(),
            "an empty request gets no answer, but got {response:?}"
        );

        let (log, result) = outcome.recv().expect("the handler should have finished");
        assert_eq!(result.expect("no error"), 0);
        assert!(
            log.contains("0 bytes from 127.0.0.1:"),
            "the log should report the end of input: {log:?}"
        );
    }

    #[test]
    fn a_request_larger_than_the_buffer_arrives_one_bufferful_at_a_time() {
        let (addr, outcome) = serve_one();
        let flood = vec![b'x'; READ_BUFFER * 2];

        let mut client = client(addr);
        // The server may already be gone by the time the last byte goes out, so this
        // write is allowed to fail too: a reset interrupts either half of the exchange.
        let _ = client.write_all(&flood);

        let mut response = Vec::new();
        let reply = client.read_to_end(&mut response);

        let (log, result) = outcome.recv().expect("the handler should have finished");
        assert_eq!(
            result.expect("no error"),
            READ_BUFFER,
            "one `read` takes one bufferful, however much is waiting"
        );
        assert!(
            log.contains(&format!("{READ_BUFFER} bytes from 127.0.0.1:")),
            "the log should say how much one read took: {log:?}"
        );

        // The other 1024 bytes are still in the kernel's receive buffer, and closing a
        // socket while bytes are unread makes the kernel send RST rather than a polite
        // FIN. The client's read therefore ends in an error instead of a clean end of
        // stream, whether or not the reply reached it first.
        assert!(
            reply.is_err(),
            "closing on unread bytes resets the connection, got {reply:?} after {response:?}"
        );
    }

    #[test]
    fn a_connection_that_cannot_be_answered_is_reported_not_swallowed() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind a test listener");
        let addr = listener
            .local_addr()
            .expect("a bound listener has an address");

        let outcome = thread::spawn(move || {
            let (stream, peer) = listener.accept().expect("accept one connection");
            // This end of the socket is already closed for writing, so the reply has
            // nowhere to go and the handler has to say so.
            stream
                .shutdown(Shutdown::Write)
                .expect("shut the write half");
            let mut log = Vec::new();
            let result = handle_connection(stream, peer, &mut log);
            (String::from_utf8_lossy(&log).into_owned(), result)
        });

        let mut client = client(addr);
        client
            .write_all(b"anyone there?\n")
            .expect("send the request");
        let mut response = Vec::new();
        // The client may see the reply, nothing at all, or a reset: the point of the test
        // is not what the client sees, but that the server does not call this a success.
        let _ = client.read_to_end(&mut response);

        let (log, result) = outcome.join().expect("the server thread should not panic");
        assert!(
            log.contains("| anyone there?"),
            "the request should still have been read and printed: {log:?}"
        );
        assert!(
            result.is_err(),
            "a socket that cannot be written to must not report success: {result:?}"
        );
    }
}
