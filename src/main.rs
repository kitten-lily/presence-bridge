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
mod steam;
#[cfg(test)]
mod testutil {
    use std::fs;
    use std::path::PathBuf;

    /// An empty directory for one test, named by its tag and the test process.
    pub fn tempdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("presence-bridge-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }
}

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
  --db FILE       use this detectable-apps JSON; never fetch or cache
  -V, --version   print the version and exit";

/// `presence-bridge 0.1.0`: the release tag without its `v`.
const VERSION: &str = concat!(env!("CARGO_PKG_NAME"), " ", env!("CARGO_PKG_VERSION"));

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
                let s: u64 = args
                    .next()
                    .and_then(|v| v.parse().ok())
                    .filter(|&s| s > 0)
                    .ok_or("--interval needs seconds > 0")?;
                o.interval = Duration::from_secs(s);
            }
            "--db" => o.db = Some(args.next().ok_or("--db needs a file")?.into()),
            "-h" | "--help" => return Err(String::new()),
            "-V" | "--version" => {
                println!("{VERSION}");
                std::process::exit(0);
            }
            _ => return Err(format!("unknown argument: {a}")),
        }
    }
    Ok(o)
}

/// One running game, and the process that stands for the whole title.
struct Game {
    name: String,
    pid: u32,
    start_ms: Option<u64>,
}

/// What detection keeps between scans about Steam launches.
#[derive(Default)]
struct SteamState {
    demos: steam::Demos,
    launches: HashMap<String, KnownLaunch>,
}

/// A title's current Steam launch, as of the last scan.
struct KnownLaunch {
    /// Its processes then. The next scan's tree is the same launch while any of
    /// them remains. Scans pause while no Discord client runs, so one launch can
    /// end and the next start without a scan seeing the title gone.
    procs: Vec<ProcKey>,
    shown: Shown,
}

/// A process's PID and start ticks, so a reused PID is another process.
type ProcKey = (u32, Option<u64>);

/// What has stood for a title during its current Steam launch.
#[derive(Clone, Copy, PartialEq)]
enum Shown {
    Executable,
    GameDir,
}

/// One title's Steam launch tree in this scan.
struct Tree<'a> {
    app: &'a db::App,
    /// The lowest PID running from the game's own directory.
    game: Option<u32>,
    /// The lowest PID with the ID: `reaper`, while it runs.
    first: u32,
    procs: Vec<ProcKey>,
}

fn detect(db: &db::Db, uid: u32, boot: Option<SystemTime>, state: &mut SteamState) -> BTreeMap<String, Game> {
    let procs = scan::processes(uid);
    let demos = &mut state.demos;
    let full_game = |l: &steam::Launch| demos.full_game(l.client.as_deref()?, l.app);
    running(db, &procs, full_game, &mut state.launches)
        .into_iter()
        .map(|(id, (app, pid))| {
            let start_ms = boot.and_then(|b| scan::start_ms(pid, b));
            (id.to_owned(), Game { name: app.name.clone(), pid, start_ms })
        })
        .collect()
}

/// Each running title by Discord app ID, with the PID that stands for it.
///
/// An executable match is the game binary itself, so it wins; the lowest PID
/// stands for the title. A Steam match covers the whole launch tree: the game
/// is the lowest PID running from the game's own directory. Until one does,
/// the lowest PID, Steam's `reaper` wrapper, stands in.
///
/// Wine's services, `reaper` and crash handlers can outlive the game, so
/// `launches` remembers, per title, what stood for it during the current
/// launch: once the executable match or the game has exited, the rest of the
/// tree doesn't keep the title shown. That is forgotten when the launch ends,
/// when none of its processes remains. `full_game` maps a demo's app ID to its
/// full game's.
fn running<'a>(
    db: &'a db::Db,
    procs: &[scan::Proc],
    mut full_game: impl FnMut(&steam::Launch) -> Option<u32>,
    launches: &mut HashMap<String, KnownLaunch>,
) -> BTreeMap<&'a str, (&'a db::App, u32)> {
    let mut found: BTreeMap<&str, (&db::App, u32)> = BTreeMap::new();
    for p in procs {
        for app in db.matches(&p.argv0, &p.args) {
            let hit = found.entry(&app.id).or_insert((app, p.pid));
            hit.1 = hit.1.min(p.pid);
        }
    }
    let mut steam: BTreeMap<&str, Tree> = BTreeMap::new();
    for p in procs {
        let Some(launch) = &p.steam else { continue };
        let Some(app) = db.steam(launch.app).or_else(|| db.steam(full_game(launch)?)) else { continue };
        let game = launch.runs_game(&p.argv0).then_some(p.pid);
        let tree = steam.entry(&app.id).or_insert(Tree { app, game, first: p.pid, procs: Vec::new() });
        tree.game = match (tree.game, game) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        tree.first = tree.first.min(p.pid);
        tree.procs.push((p.pid, p.start));
    }
    launches.retain(|id, l| steam.get(id.as_str()).is_some_and(|t| t.procs.iter().any(|p| l.procs.contains(p))));
    for (id, tree) in steam {
        let before = launches.get(id).map(|l| l.shown);
        let (pid, now) = if found.contains_key(id) {
            (None, Some(Shown::Executable))
        } else {
            match (before, tree.game) {
                // Its executable, or the game, has exited; helpers don't count.
                (Some(Shown::Executable), _) | (Some(Shown::GameDir), None) => (None, before),
                (_, Some(pid)) => (Some(pid), Some(Shown::GameDir)),
                // Not started yet: `reaper` stands in, and nothing is remembered.
                (None, None) => (Some(tree.first), None),
            }
        };
        if let Some(pid) = pid {
            found.insert(id, (tree.app, pid));
        }
        let Some(shown) = now else { continue };
        match launches.get_mut(id) {
            Some(l) => *l = KnownLaunch { procs: tree.procs, shown },
            None => {
                launches.insert(id.to_owned(), KnownLaunch { procs: tree.procs, shown });
            }
        }
    }
    found
}

fn cache_path() -> PathBuf {
    // `-v2`: from Steam matching on, the cache keeps Steam IDs. An older cache
    // has none, and would be trusted for up to a day under the old name, so it
    // is only an offline fallback now (`db::Source::cached`).
    const FILE: &str = "detectable-v2.json";
    // systemd's CacheDirectory= sets this; otherwise follow XDG.
    if let Some(d) = std::env::var_os("CACHE_DIRECTORY") {
        return PathBuf::from(d).join(FILE);
    }
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join("presence-bridge").join(FILE)
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
        None => {
            let cache = cache_path();
            let legacy = cache.with_file_name("detectable.json");
            db::Source::cached(cache, legacy, MAX_AGE)
        }
    };
    let uid = scan::own_uid();
    let boot = scan::boot_time();
    let mut steam_state = SteamState::default();

    if opts.once {
        let Some(db) = source.load() else { return ExitCode::from(1) };
        println!("detectable apps: {}", db.len());
        for c in ipc::discover(&runtime, opts.include_host) {
            let paths: Vec<_> = c.paths.iter().map(|p| p.display().to_string()).collect();
            println!("client {}: {}", c.key, paths.join(", "));
        }
        for (id, g) in detect(&db, uid, boot, &mut steam_state) {
            println!("game {} ({id}) pid {} start {:?}", g.name, g.pid, g.start_ms);
        }
        return ExitCode::SUCCESS;
    }

    let mut db: Option<db::Db> = None;
    let mut next_check = Instant::now();
    // A list refresh in flight. The fetch is ~13 MB, so it runs on its own
    // thread while sessions keep being served with the list already loaded.
    let mut refresh: Option<std::thread::JoinHandle<Option<db::Db>>> = None;
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
        if refresh.as_ref().is_some_and(|h| h.is_finished()) {
            // A panicked refresh counts as a failed one: keep the current list.
            if let Some(fresh) = refresh.take().and_then(|h| h.join().ok()).flatten() {
                eprintln!("detectable apps: {}", fresh.len());
                db = Some(fresh);
            }
            // Without any list, retry sooner than the hourly freshness check.
            next_check = Instant::now() + if db.is_some() { RECHECK } else { Duration::from_secs(300) };
        }
        if refresh.is_none() && Instant::now() >= next_check {
            if db.is_none() || source.is_stale() {
                let source = source.clone();
                refresh = Some(std::thread::spawn(move || source.load()));
            } else {
                next_check = Instant::now() + RECHECK;
            }
        }
        let Some(db) = &db else {
            std::thread::sleep(opts.interval);
            continue;
        };

        let games = detect(db, uid, boot, &mut steam_state);
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
                        eprintln!(
                            "{} (pid {}): reported to {} via {}",
                            game.name,
                            game.pid,
                            client.key,
                            s.path.display()
                        );
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

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> db::Db {
        db::Db::new(
            serde_json::from_str(
                r#"[
                {"id": "1529337689047044147", "name": "Guildrun", "executables": [],
                 "third_party_skus": [{"distributor": "steam", "id": "3669200"}]},
                {"id": "1124358970618953818", "name": "Valheim",
                 "executables": [{"name": "valheim/valheim.exe", "os": "win32"}],
                 "third_party_skus": [{"distributor": "steam", "id": "892970"}]}
            ]"#,
            )
            .unwrap(),
        )
    }

    fn proc(pid: u32, argv0: &str, app: Option<(u32, &str)>) -> scan::Proc {
        let steam = app.map(|(app, dir)| steam::Launch { app, install_dir: Some(dir.into()), client: None });
        scan::Proc { pid, argv0: argv0.into(), args: Vec::new(), steam, start: None }
    }

    /// One scan with nothing remembered from earlier ones.
    fn pids(db: &db::Db, procs: &[scan::Proc]) -> Vec<(&'static str, u32)> {
        scan(db, procs, &mut HashMap::new())
    }

    fn scan(db: &db::Db, procs: &[scan::Proc], shown: &mut HashMap<String, KnownLaunch>) -> Vec<(&'static str, u32)> {
        let names = |id: &str| if id.starts_with("1529") { "Guildrun" } else { "Valheim" };
        running(db, procs, |l| (l.app == 4425970).then_some(3669200), shown)
            .into_iter()
            .map(|(id, (_, p))| (names(id), p))
            .collect()
    }

    /// Guildrun's launch tree under Proton, as seen on 2026-10-10 (scan order is
    /// not PID order).
    fn proton(app: u32) -> Vec<scan::Proc> {
        let tag = Some((app, "guildrun demo"));
        vec![
            proc(116913, r"S:\common\Guildrun Demo\Guildrun.exe", tag),
            proc(116703, "/s/Steam/ubuntu12_32/reaper", tag),
            proc(
                116704,
                "/s/steamapps/common/SteamLinuxRuntime_4/pressure-vessel/bin/steam-runtime-launch-client",
                tag,
            ),
            proc(116792, "python3", tag),
            proc(116807, r"C:\windows\system32\services.exe", tag),
            proc(116906, r"c:\windows\system32\discord\bridge.exe", tag),
            proc(116917, r"S:\common\Guildrun Demo\UnityCrashHandler64.exe", tag),
            proc(1000, "/usr/bin/bash", None),
        ]
    }

    #[test]
    fn steam_tree_is_represented_by_the_game_not_the_launcher() {
        let db = db();
        assert_eq!(pids(&db, &proton(3669200)), [("Guildrun", 116913)]);
        let tag = Some((892970, "valheim"));
        let native = [
            proc(116324, "/s/steamapps/common/Valheim/valheim.x86_64", tag),
            proc(116246, "/s/Steam/ubuntu12_32/reaper", tag),
            proc(116248, "/s/steamapps/common/SteamLinuxRuntime_soldier/pressure-vessel/bin/launch-client", tag),
            proc(116299, "/usr/lib/pressure-vessel/from-host/libexec/steam-runtime-tools-0/pv-adverb", tag),
        ];
        assert_eq!(pids(&db, &native), [("Valheim", 116324)]);
    }

    /// The same tree without the processes in the game's directory: before the
    /// game starts, or after it and its crash handler exit.
    fn proton_without_game(app: u32) -> Vec<scan::Proc> {
        proton(app).into_iter().filter(|p| !p.argv0.starts_with("S:")).collect()
    }

    #[test]
    fn before_the_game_starts_the_wrapper_stands_in() {
        assert_eq!(pids(&db(), &proton_without_game(3669200)), [("Guildrun", 116703)]);
    }

    #[test]
    fn the_rest_of_the_tree_does_not_outlive_the_game() {
        let (db, mut shown) = (db(), HashMap::new());
        assert_eq!(scan(&db, &proton(3669200), &mut shown), [("Guildrun", 116913)]);
        // The game exited; `reaper`, Proton and Wine's services wind down.
        assert!(scan(&db, &proton_without_game(3669200), &mut shown).is_empty());
        // Once the whole tree is gone, the next launch starts over.
        assert!(scan(&db, &[], &mut shown).is_empty());
        assert_eq!(scan(&db, &proton_without_game(3669200), &mut shown), [("Guildrun", 116703)]);
    }

    #[test]
    fn a_demo_shows_as_its_full_game() {
        assert_eq!(pids(&db(), &proton(4425970)), [("Guildrun", 116913)]);
    }

    #[test]
    fn ids_not_on_the_list_match_nothing() {
        assert!(pids(&db(), &proton(1)).is_empty());
    }

    #[test]
    fn a_title_matched_both_ways_counts_once_with_the_executables_pid() {
        // Valheim on Proton: its `valheim.exe` matches by name, and the whole
        // tree, `reaper` included, carries its Steam ID.
        let tag = Some((892970, "valheim"));
        let procs = [
            proc(500, "/s/Steam/ubuntu12_32/reaper", tag),
            proc(600, r"S:\common\Valheim\valheim.exe", tag),
            proc(700, r"S:\common\Valheim\UnityCrashHandler64.exe", tag),
        ];
        assert_eq!(pids(&db(), &procs), [("Valheim", 600)]);
    }

    #[test]
    fn an_executable_match_ends_when_the_executable_exits() {
        let tag = Some((892970, "valheim"));
        let (db, mut shown) = (db(), HashMap::new());
        let reaper = || proc(500, "/s/Steam/ubuntu12_32/reaper", tag);
        let handler = || proc(700, r"S:\common\Valheim\UnityCrashHandler64.exe", tag);
        let running = [reaper(), proc(600, r"S:\common\Valheim\valheim.exe", tag), handler()];
        assert_eq!(scan(&db, &running, &mut shown), [("Valheim", 600)]);
        // `valheim.exe` exited. The crash handler is in the game's directory,
        // but it isn't the executable that stood for the title.
        assert!(scan(&db, &[reaper(), handler()], &mut shown).is_empty());
    }

    #[test]
    fn a_new_launch_starts_over_though_no_scan_saw_the_last_one_end() {
        // Scans pause while Discord is closed: Valheim through Proton, matched by
        // executable, then its native build, with no scan in between.
        let tag = Some((892970, "valheim"));
        let reaper = |pid, start| scan::Proc { start: Some(start), ..proc(pid, "/s/Steam/ubuntu12_32/reaper", tag) };
        let native = |pid| proc(pid, "/s/steamapps/common/Valheim/valheim.x86_64", tag);
        let (db, mut shown) = (db(), HashMap::new());
        let proton = [reaper(500, 1), proc(600, r"S:\common\Valheim\valheim.exe", tag)];
        assert_eq!(scan(&db, &proton, &mut shown), [("Valheim", 600)]);
        assert_eq!(scan(&db, &[reaper(900, 2), native(950)], &mut shown), [("Valheim", 950)]);
        // A new launch whose `reaper` got a reused PID: its start time differs.
        assert_eq!(scan(&db, &proton, &mut shown), [("Valheim", 600)]);
        assert_eq!(scan(&db, &[reaper(500, 3), native(550)], &mut shown), [("Valheim", 550)]);
    }

    #[test]
    fn a_launch_outlasts_its_first_process() {
        // If `reaper` exits before the last of Wine's services, that is still
        // the same launch: the exited game must not come back with a service
        // standing in for it.
        let (db, mut shown) = (db(), HashMap::new());
        assert_eq!(scan(&db, &proton(3669200), &mut shown), [("Guildrun", 116913)]);
        assert!(scan(&db, &proton_without_game(3669200), &mut shown).is_empty());
        let services: Vec<_> =
            proton_without_game(3669200).into_iter().filter(|p| !p.argv0.ends_with("/reaper")).collect();
        assert!(scan(&db, &services, &mut shown).is_empty());
    }
}
