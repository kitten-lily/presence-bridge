//! presence-bridge: Discord Rich Presence for games a sandboxed Discord can't see.
//!
//! A flatpak Discord (or Vesktop, Equibop, …) lives in its own PID namespace,
//! so its process-scan game detection never sees a game started from another
//! flatpak — World of Warcraft under Faugus shows nothing. This runs on the
//! host, scans `/proc` against Discord's detectable-games list, and reports
//! each running game over the client's IPC socket as if the game had done it.
//! See README.md.

mod db;
mod ipc;
mod scan;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant, SystemTime};

const USAGE: &str = "\
usage: presence-bridge [--once] [--interval SECS] [--include-host] [--db FILE]

  --once          scan once, print detected games and Discord clients, exit
  --interval S    seconds between scans (default 5, as arRPC)
  --include-host  also report to $XDG_RUNTIME_DIR/discord-ipc-N (a native
                  client already detects games itself; this doubles them)
  --db FILE       use this detectable-apps JSON; never fetch or cache";

/// How long a cached detectable list is trusted, and how often to check.
const MAX_AGE: Duration = Duration::from_secs(24 * 3600);
const RECHECK: Duration = Duration::from_secs(3600);

struct Opts {
    once: bool,
    interval: Duration,
    include_host: bool,
    db: Option<PathBuf>,
}

fn parse_args() -> Result<Opts, String> {
    let mut o = Opts { once: false, interval: Duration::from_secs(5), include_host: false, db: None };
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--once" => o.once = true,
            "--include-host" => o.include_host = true,
            "--interval" => {
                let s: u64 = args.next().and_then(|v| v.parse().ok()).filter(|&s| s > 0).ok_or("--interval needs seconds > 0")?;
                o.interval = Duration::from_secs(s);
            }
            "--db" => o.db = Some(args.next().ok_or("--db needs a file")?.into()),
            "-h" | "--help" => return Err(String::new()),
            _ => return Err(format!("unknown argument: {a}")),
        }
    }
    Ok(o)
}

/// One running game: the lowest matching PID stands for the whole title.
struct Game {
    name: String,
    pid: u32,
    start_ms: Option<u64>,
}

fn detect(db: &db::Db, uid: u32, boot: Option<SystemTime>) -> BTreeMap<String, Game> {
    let mut games: BTreeMap<String, Game> = BTreeMap::new();
    for p in scan::processes(uid) {
        for app in db.matches(&p.argv0, &p.args) {
            let keep = games.get(&app.id).is_none_or(|g| p.pid < g.pid);
            if keep {
                let start_ms = boot.and_then(|b| scan::start_ms(p.pid, b));
                games.insert(app.id.clone(), Game { name: app.name.clone(), pid: p.pid, start_ms });
            }
        }
    }
    games
}

fn cache_path() -> PathBuf {
    // systemd's CacheDirectory= sets this; otherwise follow XDG.
    if let Some(d) = std::env::var_os("CACHE_DIRECTORY") {
        return PathBuf::from(d).join("detectable.json");
    }
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join("presence-bridge/detectable.json")
}

fn main() -> ExitCode {
    let opts = match parse_args() {
        Ok(o) => o,
        Err(e) => {
            if !e.is_empty() {
                eprintln!("{e}");
            }
            eprintln!("{USAGE}");
            return ExitCode::from(if e.is_empty() { 0 } else { 2 });
        }
    };
    let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from) else {
        eprintln!("XDG_RUNTIME_DIR is not set; run this as a user service");
        return ExitCode::from(1);
    };
    let source = match &opts.db {
        Some(f) => db::Source::fixed(f.clone()),
        None => db::Source::cached(cache_path(), MAX_AGE),
    };
    let uid = scan::own_uid();
    let boot = scan::boot_time();

    if opts.once {
        let Some(db) = source.load() else { return ExitCode::from(1) };
        println!("detectable apps: {}", db.len());
        for c in ipc::discover(&runtime, opts.include_host) {
            let paths: Vec<_> = c.paths.iter().map(|p| p.display().to_string()).collect();
            println!("client {}: {}", c.key, paths.join(", "));
        }
        for (id, g) in detect(&db, uid, boot) {
            println!("game {} ({id}) pid {} start {:?}", g.name, g.pid, g.start_ms);
        }
        return ExitCode::SUCCESS;
    }

    let mut db: Option<db::Db> = None;
    let mut next_check = Instant::now();
    let mut sessions: HashMap<(String, String), ipc::Session> = HashMap::new();
    // Pairs whose last attempt failed, so a dead socket logs once, not every scan.
    let mut failing: HashSet<(String, String)> = HashSet::new();

    loop {
        let clients = ipc::discover(&runtime, opts.include_host);
        // No Discord running: no /proc scan and no fetch from discord.com, so
        // the service costs nothing for users who never start a client.
        if clients.is_empty() && sessions.is_empty() {
            failing.clear();
            std::thread::sleep(opts.interval);
            continue;
        }
        if Instant::now() >= next_check {
            if (db.is_none() || source.is_stale())
                && let Some(fresh) = source.load()
            {
                eprintln!("detectable apps: {}", fresh.len());
                db = Some(fresh);
            }
            // Without any list, retry sooner than the hourly freshness check.
            next_check = Instant::now() + if db.is_some() { RECHECK } else { Duration::from_secs(300) };
        }
        let Some(db) = &db else {
            std::thread::sleep(opts.interval);
            continue;
        };

        let games = detect(db, uid, boot);
        let client_keys: HashSet<&str> = clients.iter().map(|c| c.key.as_str()).collect();

        sessions.retain(|(app, key), s| {
            let Some(game) = games.get(app) else {
                // Clearing is a courtesy; closing the socket clears it anyway.
                let _ = s.set_activity(s.pid, None, true);
                eprintln!("{app}: game exited, cleared on {key}");
                return false;
            };
            if !client_keys.contains(key.as_str()) {
                eprintln!("{}: client {key} gone", game.name);
                return false;
            }
            if let Err(e) = s.pump() {
                eprintln!("{}: lost {key} ({e})", game.name);
                return false;
            }
            true
        });
        failing.retain(|(app, key)| games.contains_key(app) && client_keys.contains(key.as_str()));

        for (app, game) in &games {
            for client in &clients {
                let pair = (app.clone(), client.key.clone());
                if let Some(s) = sessions.get_mut(&pair) {
                    // Same title, new process (relaunch): restart the clock.
                    if s.pid != game.pid
                        && let Err(e) = s.set_activity(game.pid, game.start_ms, false)
                    {
                        eprintln!("{}: update on {} failed ({e})", game.name, client.key);
                        sessions.remove(&pair);
                    }
                    continue;
                }
                let res = ipc::Session::open(client, app).and_then(|mut s| {
                    s.set_activity(game.pid, game.start_ms, false)?;
                    Ok(s)
                });
                match res {
                    Ok(s) => {
                        eprintln!("{} (pid {}): reported to {} via {}", game.name, game.pid, client.key, s.path.display());
                        failing.remove(&pair);
                        sessions.insert(pair, s);
                    }
                    Err(e) => {
                        if failing.insert(pair) {
                            eprintln!("{}: cannot report to {} ({e}); retrying each scan", game.name, client.key);
                        }
                    }
                }
            }
        }

        std::thread::sleep(opts.interval);
    }
}
