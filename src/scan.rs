//! Host `/proc` scan. This is the whole reason the bridge exists: it runs in the
//! host PID namespace, so it sees games launched from any flatpak (Faugus,
//! Heroic, Steam, Lutris, Bottles) that a flatpak Discord cannot.

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::steam;

pub struct Proc {
    pub pid: u32,
    /// Wine/Proton set this to the Windows path (`C:\…\WoW.exe`); `/proc/<pid>/exe`
    /// would only name the wine preloader.
    pub argv0: String,
    pub args: Vec<String>,
    /// Set when Steam launched this process or an ancestor.
    pub steam: Option<steam::Launch>,
    /// Start time in clock ticks after boot; read for Steam's processes only,
    /// to tell a reused PID from the process that had it before.
    pub start: Option<u64>,
}

/// Every process owned by `uid` that has a command line (kernel threads don't).
pub fn processes(uid: u32) -> Vec<Proc> {
    let Ok(dir) = fs::read_dir("/proc") else { return Vec::new() };
    dir.filter_map(Result::ok)
        .filter_map(|ent| {
            let pid: u32 = ent.file_name().to_str()?.parse().ok()?;
            if ent.metadata().ok()?.uid() != uid {
                return None;
            }
            let path = ent.path();
            let raw = fs::read(path.join("cmdline")).ok()?;
            let mut it = raw.split(|&b| b == 0).map(|s| String::from_utf8_lossy(s).into_owned());
            let argv0 = it.next().filter(|s| !s.is_empty())?;
            let mut args: Vec<String> = it.collect();
            // cmdline is NUL-terminated, so split yields a trailing "".
            if args.last().is_some_and(String::is_empty) {
                args.pop();
            }
            // Some of the user's own are unreadable (systemd --user, ssh-agent):
            // those just aren't Steam games.
            let steam = fs::read(path.join("environ")).ok().and_then(|e| steam::Launch::from_environ(&e));
            let start = steam.as_ref().and_then(|_| start_ticks(pid));
            Some(Proc { pid, argv0, args, steam, start })
        })
        .collect()
}

pub fn own_uid() -> u32 {
    fs::metadata("/proc/self").map(|m| m.uid()).unwrap_or(u32::MAX)
}

/// Boot time, for turning a process start tick into wall-clock time.
pub fn boot_time() -> Option<SystemTime> {
    let stat = fs::read_to_string("/proc/stat").ok()?;
    let secs: u64 = stat.lines().find_map(|l| l.strip_prefix("btime "))?.trim().parse().ok()?;
    Some(UNIX_EPOCH + Duration::from_secs(secs))
}

/// When `pid` started, in clock ticks after boot.
fn start_ticks(pid: u32) -> Option<u64> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // comm (field 2) may contain spaces and parens; fields resume after the last ')'.
    // starttime is field 22 overall, i.e. index 19 counting from field 3.
    stat.rsplit_once(')')?.1.split_whitespace().nth(19)?.parse().ok()
}

/// When `pid` started, in Unix milliseconds, so elapsed time is right even if
/// the bridge started after the game.
pub fn start_ms(pid: u32, boot: SystemTime) -> Option<u64> {
    // USER_HZ is fixed at 100 in the Linux ABI on x86_64 and aarch64; this is
    // what sysconf(_SC_CLK_TCK) returns.
    const USER_HZ: u64 = 100;
    let start = boot + Duration::from_millis(start_ticks(pid)? * 1000 / USER_HZ);
    Some(start.duration_since(UNIX_EPOCH).ok()?.as_millis() as u64)
}
