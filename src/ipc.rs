//! Discord IPC: finding sandboxed clients' sockets and talking to them.
//!
//! Wire format: `u32 op` + `u32 len` (little-endian) + `len` bytes of JSON.
//! One connection is bound to one application ID by its handshake, so each
//! (game, client) pair gets its own [`Session`]. Discord clears the activity
//! when that connection closes, which is how a game exit is reported.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::fs::FileTypeExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

const OP_HANDSHAKE: u32 = 0;
const OP_FRAME: u32 = 1;
const OP_CLOSE: u32 = 2;
const OP_PING: u32 = 3;
const OP_PONG: u32 = 4;

/// Discord's own client caps a frame well below this; a larger length means
/// the stream is out of sync, not a big message.
const MAX_FRAME: usize = 1 << 20;
/// Time a handshake or `SET_ACTIVITY` gets, end to end: sending the request,
/// answering pings, and reading the reply. Neither pings nor a peer that
/// trickles bytes in or drains them out slowly extend it.
const TIMEOUT: Duration = Duration::from_secs(3);
/// Bytes one [`Session::pump`] reads: a peer that never stops sending gets
/// one maximal frame's worth per scan instead of holding the loop.
const PUMP_BUDGET: usize = MAX_FRAME + 8;
/// Largest single `write` in [`Session::send`]: see its doc for why sends are
/// chunked.
const WRITE_CHUNK: usize = 4096;

/// A Discord client reachable from the host, keyed by its flatpak app ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Client {
    pub key: String,
    /// Candidate sockets, best first. A flatpak Discord has two: its real
    /// socket under `.flatpak/<id>/xdg-run/` and the `socat` relay its wrapper
    /// puts in `app/<id>/`.
    pub paths: Vec<PathBuf>,
}

/// Every Discord-ish IPC socket a sandbox exposes on the host.
///
/// Probes the per-app locations Proton's `rpc-bridge` probes, generalised to
/// any app ID so Discord, Vesktop, Equibop, Legcord, … all work without a list.
///
/// The canonical `$XDG_RUNTIME_DIR/discord-ipc-N` is skipped unless
/// `include_host`: a client listening there runs in the host PID namespace and
/// detects games itself, so reporting to it would double the activity.
pub fn discover(runtime: &Path, include_host: bool) -> Vec<Client> {
    let mut found: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    for (dir, sub) in [(".flatpak", Some("xdg-run")), ("app", None)] {
        let Ok(entries) = fs::read_dir(runtime.join(dir)) else { continue };
        for ent in entries.filter_map(Result::ok) {
            let Some(id) = ent.file_name().to_str().map(str::to_owned) else { continue };
            let base = match sub {
                Some(s) => ent.path().join(s),
                None => ent.path(),
            };
            let socks = sockets_in(&base);
            if !socks.is_empty() {
                found.entry(id).or_default().extend(socks);
            }
        }
    }
    if include_host {
        let socks = sockets_in(runtime);
        if !socks.is_empty() {
            found.insert("host".into(), socks);
        }
    }
    found.into_iter().map(|(key, paths)| Client { key, paths }).collect()
}

/// `discord-ipc-0` … `discord-ipc-9`, real sockets only.
///
/// Flatpak leaves an empty regular file at `.flatpak/<id>/xdg-run/discord-ipc-N`
/// in every app granted that path, so only the file type tells a stub from a
/// client. Links are not followed.
fn sockets_in(dir: &Path) -> Vec<PathBuf> {
    (0..10)
        .map(|n| dir.join(format!("discord-ipc-{n}")))
        .filter(|p| fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_socket()))
        .collect()
}

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    /// The client answered the handshake with CLOSE (e.g. 4000 invalid client_id).
    Rejected(Value),
    Protocol(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(e) => write!(f, "{e}"),
            Error::Rejected(v) => write!(f, "rejected: {v}"),
            Error::Protocol(s) => write!(f, "protocol: {s}"),
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Error {
        Error::Io(e)
    }
}

pub struct Session {
    stream: UnixStream,
    buf: Vec<u8>,
    nonce: u64,
    pub path: PathBuf,
    /// The game process the current activity describes.
    pub pid: u32,
}

impl Session {
    /// Connect to the first live socket of `client` and handshake as `app_id`.
    pub fn open(client: &Client, app_id: &str) -> Result<Session, Error> {
        let mut last = Error::Protocol("no socket".into());
        for path in &client.paths {
            match Session::open_path(path, app_id) {
                Ok(s) => return Ok(s),
                Err(e @ Error::Rejected(_)) => return Err(e),
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    fn open_path(path: &Path, app_id: &str) -> Result<Session, Error> {
        let stream = UnixStream::connect(path)?;
        let mut s = Session { stream, buf: Vec::new(), nonce: 0, path: path.to_owned(), pid: 0 };
        let deadline = Instant::now() + TIMEOUT;
        s.send(OP_HANDSHAKE, &json!({ "v": 1, "client_id": app_id }), deadline)?;
        let (op, msg) = s.recv_blocking(deadline)?;
        match op {
            OP_FRAME if msg["evt"] == "READY" => Ok(s),
            OP_CLOSE => Err(Error::Rejected(msg)),
            _ => Err(Error::Protocol(format!("unexpected handshake reply op={op} {msg}"))),
        }
    }

    /// Set (or with `None`, clear) the activity and wait for Discord's reply.
    ///
    /// The activity carries only a start time: the client fills in the game's
    /// name and icon from the application ID bound by the handshake.
    pub fn set_activity(&mut self, pid: u32, start_ms: Option<u64>, clear: bool) -> Result<(), Error> {
        self.nonce += 1;
        let nonce = self.nonce.to_string();
        let activity = if clear {
            Value::Null
        } else {
            match start_ms {
                Some(ms) => json!({ "timestamps": { "start": ms } }),
                None => json!({}),
            }
        };
        let deadline = Instant::now() + TIMEOUT;
        self.send(
            OP_FRAME,
            &json!({ "cmd": "SET_ACTIVITY", "args": { "pid": pid, "activity": activity }, "nonce": nonce }),
            deadline,
        )?;
        loop {
            let (op, msg) = self.recv_blocking(deadline)?;
            match op {
                OP_FRAME if msg["nonce"] == nonce.as_str() => {
                    if msg["evt"] == "ERROR" {
                        return Err(Error::Rejected(msg["data"].clone()));
                    }
                    self.pid = pid;
                    return Ok(());
                }
                OP_CLOSE => return Err(Error::Rejected(msg)),
                _ => {}
            }
        }
    }

    /// Between scans: answer pings, discard unsolicited frames, and report
    /// whether the client is still there. Never blocks, and reads at most
    /// [`PUMP_BUDGET`] bytes.
    pub fn pump(&mut self) -> Result<(), Error> {
        self.stream.set_nonblocking(true)?;
        let res = self.drain_available();
        self.stream.set_nonblocking(false)?;
        res
    }

    /// Frames are handled as soon as they are complete, so the buffer never
    /// holds more than one frame plus one read.
    fn drain_available(&mut self) -> Result<(), Error> {
        let mut chunk = [0u8; 4096];
        let mut budget = PUMP_BUDGET;
        loop {
            while let Some((op, msg)) = self.take_frame()? {
                self.handle_unsolicited(op, msg)?;
            }
            if budget == 0 {
                return Ok(());
            }
            let want = budget.min(chunk.len());
            match self.stream.read(&mut chunk[..want]) {
                Ok(0) => return Err(Error::Io(io::ErrorKind::UnexpectedEof.into())),
                Ok(n) => {
                    self.buf.extend_from_slice(&chunk[..n]);
                    budget -= n;
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e.into()),
            }
        }
    }

    fn handle_unsolicited(&mut self, op: u32, msg: Value) -> Result<(), Error> {
        match op {
            // Called with the socket non-blocking, so a peer that isn't reading
            // fails this at once rather than after TIMEOUT.
            OP_PING => self.send(OP_PONG, &msg, Instant::now() + TIMEOUT),
            OP_CLOSE => Err(Error::Rejected(msg)),
            _ => Ok(()),
        }
    }

    /// Next frame, or a timeout once `deadline` passes. Pings are answered
    /// here too, so a ping arriving mid-request is not mistaken for the reply;
    /// they do not buy the peer more time.
    fn recv_blocking(&mut self, deadline: Instant) -> Result<(u32, Value), Error> {
        loop {
            if let Some((op, msg)) = self.take_frame()? {
                if op == OP_PING {
                    self.send(OP_PONG, &msg, deadline)?;
                    continue;
                }
                return Ok((op, msg));
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(timed_out());
            }
            self.stream.set_read_timeout(Some(left))?;
            let mut chunk = [0u8; 4096];
            match self.stream.read(&mut chunk) {
                Ok(0) => return Err(Error::Io(io::ErrorKind::UnexpectedEof.into())),
                Ok(n) => self.buf.extend_from_slice(&chunk[..n]),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                // An expired read timeout is EAGAIN on Linux, not ETIMEDOUT.
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    return Err(timed_out());
                }
                Err(e) => return Err(e.into()),
            }
        }
    }

    fn take_frame(&mut self) -> Result<Option<(u32, Value)>, Error> {
        if self.buf.len() < 8 {
            return Ok(None);
        }
        let op = u32::from_le_bytes(self.buf[0..4].try_into().unwrap());
        let len = u32::from_le_bytes(self.buf[4..8].try_into().unwrap()) as usize;
        if len > MAX_FRAME {
            return Err(Error::Protocol(format!("frame length {len}")));
        }
        if self.buf.len() < 8 + len {
            return Ok(None);
        }
        let msg = serde_json::from_slice(&self.buf[8..8 + len])
            .map_err(|e| Error::Protocol(format!("bad JSON: {e}")))?;
        self.buf.drain(..8 + len);
        Ok(Some((op, msg)))
    }

    /// Write one frame, or time out once `deadline` passes. A pong echoes the
    /// ping, so a near-`MAX_FRAME` ping yields a pong the socket can't hold, and
    /// the peer decides how fast it drains.
    ///
    /// Linux applies a Unix socket's `SO_SNDTIMEO` per internal buffer
    /// allocation, not per `write`, so one large `write` blocks for as long as
    /// a slow reader keeps freeing buffers. Writing at most [`WRITE_CHUNK`]
    /// bytes per call, with the timeout re-armed to the time remaining, keeps
    /// each call short enough for the deadline check between them to bind.
    fn send(&mut self, op: u32, msg: &Value, deadline: Instant) -> Result<(), Error> {
        let body = serde_json::to_vec(msg).map_err(|e| Error::Protocol(e.to_string()))?;
        let mut frame = Vec::with_capacity(8 + body.len());
        frame.extend_from_slice(&op.to_le_bytes());
        frame.extend_from_slice(&(body.len() as u32).to_le_bytes());
        frame.extend_from_slice(&body);
        let mut rest = &frame[..];
        while !rest.is_empty() {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(timed_out());
            }
            self.stream.set_write_timeout(Some(left))?;
            match self.stream.write(&rest[..rest.len().min(WRITE_CHUNK)]) {
                Ok(0) => return Err(Error::Io(io::ErrorKind::WriteZero.into())),
                Ok(n) => rest = &rest[n..],
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                // An expired write timeout is EAGAIN on Linux, like reads.
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Err(timed_out()),
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }
}

fn timed_out() -> Error {
    Error::Io(io::ErrorKind::TimedOut.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    use std::thread;

    fn tempdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("presence-bridge-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn read_frame(s: &mut UnixStream) -> Option<(u32, Value)> {
        let mut hdr = [0u8; 8];
        s.read_exact(&mut hdr).ok()?;
        let op = u32::from_le_bytes(hdr[0..4].try_into().unwrap());
        let len = u32::from_le_bytes(hdr[4..8].try_into().unwrap()) as usize;
        let mut body = vec![0u8; len];
        s.read_exact(&mut body).ok()?;
        Some((op, serde_json::from_slice(&body).unwrap()))
    }

    fn write_frame(s: &mut UnixStream, op: u32, v: &Value) {
        let body = serde_json::to_vec(v).unwrap();
        s.write_all(&op.to_le_bytes()).unwrap();
        s.write_all(&(body.len() as u32).to_le_bytes()).unwrap();
        s.write_all(&body).unwrap();
    }

    #[test]
    fn discover_keys_by_app_and_prefers_real_socket_over_relay() {
        let rt = tempdir("discover");
        let real = rt.join(".flatpak/com.discordapp.Discord/xdg-run");
        let relay = rt.join("app/com.discordapp.Discord");
        let stub = rt.join(".flatpak/io.github.Faugus.faugus-launcher/xdg-run");
        for d in [&real, &relay, &stub] {
            fs::create_dir_all(d).unwrap();
        }
        let _a = UnixListener::bind(real.join("discord-ipc-0")).unwrap();
        let _b = UnixListener::bind(relay.join("discord-ipc-0")).unwrap();
        let _c = UnixListener::bind(rt.join("discord-ipc-0")).unwrap();
        fs::write(stub.join("discord-ipc-0"), "").unwrap();

        let found = discover(&rt, false);
        assert_eq!(
            found,
            [Client {
                key: "com.discordapp.Discord".into(),
                paths: vec![real.join("discord-ipc-0"), relay.join("discord-ipc-0")],
            }]
        );
        assert!(discover(&rt, true).iter().any(|c| c.key == "host"));
        fs::remove_dir_all(rt).unwrap();
    }

    #[test]
    fn session_handshakes_sets_activity_and_sees_disconnect() {
        let rt = tempdir("session");
        let sock = rt.join("discord-ipc-0");
        let listener = UnixListener::bind(&sock).unwrap();
        let server = thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let (op, hs) = read_frame(&mut s).unwrap();
            assert_eq!((op, hs["client_id"].as_str()), (OP_HANDSHAKE, Some("356875762940379136")));
            write_frame(&mut s, OP_FRAME, &json!({ "cmd": "DISPATCH", "evt": "READY", "data": {} }));
            // A ping before the reply must be answered, not taken as the reply.
            write_frame(&mut s, OP_PING, &json!({ "x": 1 }));
            let (op, req) = read_frame(&mut s).unwrap();
            assert_eq!(op, OP_FRAME);
            assert_eq!(req["args"]["activity"]["timestamps"]["start"], 1234);
            assert_eq!(req["args"]["pid"], 42);
            let (op, pong) = read_frame(&mut s).unwrap();
            assert_eq!((op, pong), (OP_PONG, json!({ "x": 1 })));
            write_frame(&mut s, OP_FRAME, &json!({ "cmd": "SET_ACTIVITY", "evt": null, "nonce": req["nonce"], "data": {} }));
            // Dropping `s` closes the connection, as a quitting Discord would.
        });

        let client = Client { key: "test".into(), paths: vec![sock] };
        let mut s = Session::open(&client, "356875762940379136").unwrap();
        s.set_activity(42, Some(1234), false).unwrap();
        assert_eq!(s.pid, 42);
        server.join().unwrap();
        thread::sleep(Duration::from_millis(50));
        assert!(s.pump().is_err());
        fs::remove_dir_all(rt).unwrap();
    }

    #[test]
    fn handshake_close_is_a_rejection() {
        let rt = tempdir("reject");
        let sock = rt.join("discord-ipc-0");
        let listener = UnixListener::bind(&sock).unwrap();
        let server = thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            read_frame(&mut s).unwrap();
            write_frame(&mut s, OP_CLOSE, &json!({ "code": 4000, "message": "Invalid Client ID" }));
        });
        let client = Client { key: "test".into(), paths: vec![sock] };
        assert!(matches!(Session::open(&client, "0"), Err(Error::Rejected(v)) if v["code"] == 4000));
        server.join().unwrap();
        fs::remove_dir_all(rt).unwrap();
    }

    /// Run `f` on its own thread; a hang fails the test instead of the suite.
    fn within<T: Send + 'static>(limit: Duration, f: impl FnOnce() -> T + Send + 'static) -> T {
        let (tx, rx) = std::sync::mpsc::channel();
        thread::spawn(move || {
            let _ = tx.send(f());
        });
        rx.recv_timeout(limit).expect("still blocked")
    }

    #[test]
    fn pings_do_not_extend_the_handshake_deadline() {
        let rt = tempdir("ping-flood");
        let sock = rt.join("discord-ipc-0");
        let listener = UnixListener::bind(&sock).unwrap();
        thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            read_frame(&mut s).unwrap();
            // Never READY, just a ping every 100 ms until the bridge hangs up.
            loop {
                let body = b"{}";
                let mut f = OP_PING.to_le_bytes().to_vec();
                f.extend_from_slice(&(body.len() as u32).to_le_bytes());
                f.extend_from_slice(body);
                if s.write_all(&f).is_err() {
                    return;
                }
                thread::sleep(Duration::from_millis(100));
            }
        });
        let client = Client { key: "test".into(), paths: vec![sock] };
        let res = within(TIMEOUT * 3, move || Session::open(&client, "1").map(|_| ()));
        assert!(matches!(res, Err(Error::Io(e)) if e.kind() == io::ErrorKind::TimedOut));
        fs::remove_dir_all(rt).unwrap();
    }

    #[test]
    fn a_slow_reader_does_not_extend_the_deadline_through_pong_writes() {
        let rt = tempdir("slow-reader");
        let sock = rt.join("discord-ipc-0");
        let listener = UnixListener::bind(&sock).unwrap();
        thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            read_frame(&mut s).unwrap();
            // One ping near MAX_FRAME, far larger than the socket buffer: the pong
            // echoes it, so writing it blocks until this side reads. Reading
            // 16 KiB every 200 ms keeps each write() making progress, so a
            // per-call write timeout never fires; only a deadline stops it.
            write_frame(&mut s, OP_PING, &json!({ "pad": "x".repeat(1000 * 1024) }));
            let mut chunk = [0u8; 16 * 1024];
            while s.read(&mut chunk).is_ok_and(|n| n > 0) {
                thread::sleep(Duration::from_millis(200));
            }
        });
        let client = Client { key: "test".into(), paths: vec![sock] };
        let res = within(TIMEOUT * 3, move || Session::open(&client, "1").map(|_| ()));
        assert!(matches!(res, Err(Error::Io(e)) if e.kind() == io::ErrorKind::TimedOut));
        fs::remove_dir_all(rt).unwrap();
    }

    #[test]
    fn pump_returns_while_the_peer_keeps_sending() {
        let rt = tempdir("flood");
        let sock = rt.join("discord-ipc-0");
        let listener = UnixListener::bind(&sock).unwrap();
        let (flooding_tx, flooding) = std::sync::mpsc::channel();
        thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            read_frame(&mut s).unwrap();
            write_frame(&mut s, OP_FRAME, &json!({ "evt": "READY" }));
            let (_, req) = read_frame(&mut s).unwrap();
            write_frame(&mut s, OP_FRAME, &json!({ "nonce": req["nonce"], "evt": null }));
            // Valid unsolicited frames, 64 KiB per write, until the bridge hangs up.
            let mut one = OP_FRAME.to_le_bytes().to_vec();
            one.extend_from_slice(&2u32.to_le_bytes());
            one.extend_from_slice(b"{}");
            let burst: Vec<u8> = one.iter().copied().cycle().take(one.len() * 6554).collect();
            s.write_all(&burst).unwrap();
            flooding_tx.send(()).unwrap();
            while s.write_all(&burst).is_ok() {}
        });
        let client = Client { key: "test".into(), paths: vec![sock] };
        let mut s = Session::open(&client, "1").unwrap();
        s.set_activity(1, None, false).unwrap();
        flooding.recv().unwrap();
        assert!(within(Duration::from_secs(5), move || s.pump().is_ok()));
        fs::remove_dir_all(rt).unwrap();
    }
}
