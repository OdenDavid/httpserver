//! End-to-end tests: the real binary, over a real socket.
//!
//! These start `target/debug/rustserver`, read its greeting to learn the port, talk to it
//! over a socket, and check what it printed. The unit tests in `src/main.rs` call
//! `handle_connection` directly; these cover the greeting, the accept loop and exit codes.

use std::fs;
use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread::sleep;
use std::time::{Duration, Instant};

/// The binary under test; Cargo builds it before these tests run.
const BIN: &str = env!("CARGO_BIN_EXE_rustserver");

/// How long a test waits before failing instead of hanging.
const PATIENCE: Duration = Duration::from_secs(10);

/// A running `rustserver`, with its stdout and stderr in one file.
///
/// A file and not a pipe: a server's output has no end, and a pipe nobody drains fills up
/// and blocks the child.
struct Server {
    child: Child,
    log_path: PathBuf,
    /// Where to connect; found in the greeting, because the kernel picks the port.
    addr: SocketAddr,
}

impl Server {
    /// Starts the binary on a free port and waits until it says which one it got.
    fn start() -> Server {
        let log_path = log_path();
        let log = fs::File::create(&log_path).expect("create the log file");
        let child = Command::new(BIN)
            // Port 0: the kernel picks a free port, so tests can run in parallel.
            .arg("127.0.0.1:0")
            .stdin(Stdio::null())
            .stdout(Stdio::from(log.try_clone().expect("clone the log file")))
            .stderr(Stdio::from(log))
            .spawn()
            .expect("start rustserver");

        let addr = wait_for_addr(&log_path);
        Server {
            child,
            log_path,
            addr,
        }
    }

    /// Everything printed so far.
    fn log(&self) -> String {
        fs::read_to_string(&self.log_path).unwrap_or_default()
    }

    /// Waits until the server has printed `needle`.
    fn wait_for_log(&self, needle: &str) {
        wait_for_log(&self.log_path, needle);
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_file(&self.log_path);
    }
}

/// Where one test writes its transcript: unique per process, and per call within it.
fn log_path() -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("rustserver-test-{}-{n}.log", std::process::id()))
}

/// Waits for the greeting and returns the address inside it.
fn wait_for_addr(log_path: &Path) -> SocketAddr {
    let deadline = Instant::now() + PATIENCE;
    loop {
        let log = fs::read_to_string(log_path).unwrap_or_default();
        if let Some(line) = log.lines().find(|line| line.contains("listening on")) {
            let addr = line
                .rsplit_once(' ')
                .expect("an address after the last space")
                .1;
            return addr.parse().expect("the greeting ends in a socket address");
        }
        assert!(
            Instant::now() < deadline,
            "no greeting within {PATIENCE:?}; the server printed {log:?}"
        );
        sleep(Duration::from_millis(10));
    }
}

/// Waits until the transcript contains `needle`, or fails the test saying what it saw.
fn wait_for_log(log_path: &Path, needle: &str) {
    let deadline = Instant::now() + PATIENCE;
    loop {
        let log = fs::read_to_string(log_path).unwrap_or_default();
        if log.contains(needle) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "never saw {needle:?} within {PATIENCE:?}; the server printed {log:?}"
        );
        sleep(Duration::from_millis(10));
    }
}

/// Runs the binary with `args` to completion: its exit code, and everything it printed.
fn run_to_completion(args: &[&str]) -> (Option<i32>, String) {
    let log_path = log_path();
    let log = fs::File::create(&log_path).expect("create the log file");
    let status = Command::new(BIN)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log.try_clone().expect("clone the log file")))
        .stderr(Stdio::from(log))
        .status()
        .expect("run rustserver");

    let printed = fs::read_to_string(&log_path).unwrap_or_default();
    let _ = fs::remove_file(&log_path);
    (status.code(), printed)
}

/// The five V1 requirements, against the real program: it says where it listens, accepts,
/// reads, prints what it read, answers, and closes.
#[test]
fn the_server_reads_prints_answers_and_closes() {
    let server = Server::start();
    let request = b"GET /nothing-http-yet\n";

    let mut client = TcpStream::connect(server.addr).expect("connect to the server");
    // A timeout, not the default of waiting forever: a test that hangs says nothing.
    client
        .set_read_timeout(Some(PATIENCE))
        .expect("set a read timeout");
    client.write_all(request).expect("send a request");

    // Reads until the server closes: if it never closed, this fails instead of passing.
    let mut response = Vec::new();
    client
        .read_to_end(&mut response)
        .expect("read the response");
    assert!(
        String::from_utf8_lossy(&response).starts_with("rustserver v1:"),
        "unexpected response: {response:?}"
    );

    server.wait_for_log("| GET /nothing-http-yet");
    let log = server.log();
    assert!(
        log.contains(&format!("{} bytes from", request.len())),
        "the count of bytes read should be printed: {log:?}"
    );
    assert!(
        log.contains("sent 44 bytes, closing"),
        "the reply should be reported as sent: {log:?}"
    );
}

/// A client that connects and says nothing must not stop the next client being served.
#[test]
fn a_silent_client_does_not_stop_the_server_serving_the_next_one() {
    let server = Server::start();

    // Half-close and leave: the server's `read` returns 0 and goes back to accepting.
    let silent = TcpStream::connect(server.addr).expect("connect and say nothing");
    silent.shutdown(Shutdown::Write).expect("half-close");
    drop(silent);
    server.wait_for_log("0 bytes from");

    let mut client = TcpStream::connect(server.addr).expect("connect again");
    client
        .set_read_timeout(Some(PATIENCE))
        .expect("set a read timeout");
    client.write_all(b"still there?\n").expect("send a request");

    let mut response = Vec::new();
    client
        .read_to_end(&mut response)
        .expect("read the response");
    assert!(
        !response.is_empty(),
        "the second client should have been answered"
    );

    server.wait_for_log("| still there?");
}

#[test]
fn help_prints_the_usage_and_exits_zero() {
    let (code, printed) = run_to_completion(&["--help"]);

    assert_eq!(code, Some(0));
    assert!(
        printed.contains("rustserver [address]"),
        "the usage should be printed: {printed:?}"
    );
}

#[test]
fn a_port_already_in_use_is_reported_and_exits_one() {
    // A real listener on a real port, held open for the length of this test.
    let taken = TcpListener::bind("127.0.0.1:0").expect("bind a port to occupy");
    let addr = taken.local_addr().expect("a bound listener has an address");

    let (code, printed) = run_to_completion(&[&addr.to_string()]);

    assert_eq!(code, Some(1));
    assert!(
        printed.contains(&format!("cannot listen on {addr}")),
        "the failure should name the address: {printed:?}"
    );
}

#[test]
fn a_wrong_command_line_exits_two() {
    let (code, printed) = run_to_completion(&["127.0.0.1:0", "extra"]);

    assert_eq!(code, Some(2));
    assert!(
        printed.contains("the command line was wrong"),
        "the failure should say what was wrong: {printed:?}"
    );
    assert!(
        printed.contains("rustserver [address]"),
        "the usage should follow it: {printed:?}"
    );
}
