//! Steam: which game a process belongs to, from the environment Steam gives its
//! whole launch tree, and which full game a demo belongs to, from Steam's local
//! app-info cache. `docs/skills/detection.md` § Steam's environment.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// A Steam app ID: decimal digits, not zero. Rejects umu-launcher's
/// `SteamAppId=default`.
pub fn app_id(s: &str) -> Option<u32> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok().filter(|&id| id != 0)
}

/// What Steam put in a process's environment. Every process of a launch carries
/// it: Steam's `reaper` wrapper, pressure-vessel, Proton, Wine's services and
/// the game, so it says which game, not which process is the game.
#[derive(Debug, Clone, PartialEq)]
pub struct Launch {
    pub app: u32,
    /// The game's directory under `steamapps/common/`, lowercased: the last
    /// component of `STEAM_COMPAT_INSTALL_PATH`.
    pub install_dir: Option<String>,
    /// Steam's own install (`STEAM_COMPAT_CLIENT_INSTALL_PATH`), which holds
    /// `appcache/appinfo.vdf`.
    pub client: Option<PathBuf>,
}

impl Launch {
    /// Reads a `/proc/<pid>/environ`. `None` unless `SteamAppId` is an app ID.
    pub fn from_environ(environ: &[u8]) -> Option<Launch> {
        let (mut app, mut install, mut client) = (None, None, None);
        for var in environ.split(|&b| b == 0) {
            if let Some(v) = var.strip_prefix(b"SteamAppId=") {
                app = std::str::from_utf8(v).ok().and_then(app_id);
            } else if let Some(v) = var.strip_prefix(b"STEAM_COMPAT_INSTALL_PATH=") {
                install = Some(v);
            } else if let Some(v) = var.strip_prefix(b"STEAM_COMPAT_CLIENT_INSTALL_PATH=") {
                client = Some(v);
            }
        }
        let install_dir = install.and_then(|p| {
            let p = String::from_utf8_lossy(p);
            p.trim_end_matches('/').rsplit('/').next().filter(|d| !d.is_empty()).map(str::to_lowercase)
        });
        Some(Launch {
            app: app?,
            install_dir,
            client: client.filter(|c| !c.is_empty()).map(|c| PathBuf::from(OsStr::from_bytes(c))),
        })
    }

    /// Whether `argv0` runs from the game's own directory: a native build's
    /// `…/steamapps/common/<dir>/…`, or Proton's `S:\common\<dir>\…` (Proton maps
    /// `steamapps/` to `S:`). The rest of the launch tree doesn't, including
    /// pressure-vessel's launch client, which sits under `steamapps/common/` too,
    /// in `SteamLinuxRuntime_*`.
    pub fn runs_game(&self, argv0: &str) -> bool {
        let Some(dir) = &self.install_dir else { return false };
        let path = argv0.to_lowercase().replace('\\', "/");
        path.match_indices("/common/")
            .any(|(i, m)| path[i + m.len()..].strip_prefix(dir.as_str()).is_some_and(|rest| rest.starts_with('/')))
    }
}

/// Demo → full game, from a Steam install's `appcache/appinfo.vdf`. A demo has
/// its own app ID, and Discord's list carries only the full game's. The file is
/// parsed again only when it changes.
#[derive(Default)]
pub struct Demos {
    file: Option<AppInfo>,
    parents: HashMap<u32, u32>,
}

struct AppInfo {
    client: PathBuf,
    path: PathBuf,
    /// The mtime `parents` was read at; `None` before the first read.
    mtime: Option<SystemTime>,
}

impl Demos {
    /// The full game's app ID, if this Steam install's cache says `app` is a demo.
    pub fn full_game(&mut self, client: &Path, app: u32) -> Option<u32> {
        if self.file.as_ref().is_none_or(|f| f.client != client) {
            let path = client.join("appcache/appinfo.vdf");
            self.file = Some(AppInfo { client: client.to_owned(), path, mtime: None });
            self.parents.clear();
        }
        let file = self.file.as_mut()?;
        let mtime = fs::metadata(&file.path).and_then(|m| m.modified()).ok()?;
        if file.mtime != Some(mtime) {
            self.parents = fs::read(&file.path).map(|b| demo_parents(&b)).unwrap_or_default();
            file.mtime = Some(mtime);
        }
        self.parents.get(&app).copied()
    }
}

const V28: u32 = 0x0756_4428;
/// v29 moved keys into a string table at the end of the file.
const V29: u32 = 0x0756_4429;
/// After an entry's app ID and size: info state, last updated, PICS token,
/// SHA-1, change number, binary SHA-1.
const ENTRY_HEADER: usize = 4 + 4 + 8 + 20 + 4 + 20;
const MAX_DEPTH: usize = 32;

/// Every demo in an `appinfo.vdf` (v28 or v29), mapped to its
/// `appinfo/common/parent`. Damage ends the walk, keeping what was read before.
fn demo_parents(b: &[u8]) -> HashMap<u32, u32> {
    let mut out = HashMap::new();
    let Some(magic) = le_u32(b, 0) else { return out };
    let (strings, mut at) = match magic {
        V28 => (None, 8),
        V29 => {
            let Some(table) = b.get(8..16).and_then(|s| usize::try_from(u64::from_le_bytes(s.try_into().ok()?)).ok())
            else {
                return out;
            };
            let Some(strings) = string_table(b, table) else { return out };
            (Some(strings), 16)
        }
        _ => {
            eprintln!("appinfo.vdf: unknown format {magic:#x}; demos are not resolved");
            return out;
        }
    };
    while let (Some(app), Some(size)) = (le_u32(b, at), le_u32(b, at + 4)) {
        let start = at + 8;
        let end = start + size as usize;
        if app == 0 || size as usize <= ENTRY_HEADER || end > b.len() {
            break;
        }
        let mut r = Reader { b: &b[..end], i: start + ENTRY_HEADER, strings: strings.as_deref() };
        let mut common = Common::default();
        if r.map(0, true, &mut common).is_none() {
            break;
        }
        if common.demo
            && let Some(parent) = common.parent
        {
            out.insert(app, parent);
        }
        at = end;
    }
    out
}

fn le_u32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at.checked_add(4)?)?.try_into().ok()?))
}

/// v29's key table: a count, then that many NUL-terminated strings.
fn string_table(b: &[u8], at: usize) -> Option<Vec<&[u8]>> {
    let count = le_u32(b, at)? as usize;
    let mut rest = b.get(at + 4..)?;
    // Each string takes at least its NUL, so a count beyond that is damage.
    let mut out = Vec::with_capacity(count.min(rest.len()));
    for _ in 0..count {
        let n = rest.iter().position(|&c| c == 0)?;
        out.push(&rest[..n]);
        rest = &rest[n + 1..];
    }
    Some(out)
}

#[derive(Default)]
struct Common {
    demo: bool,
    parent: Option<u32>,
}

/// One entry's binary KeyValues.
struct Reader<'a, 's> {
    b: &'a [u8],
    i: usize,
    /// v29's key table; v28 writes keys inline.
    strings: Option<&'s [&'a [u8]]>,
}

impl<'a> Reader<'a, '_> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.b.get(self.i..self.i.checked_add(n)?)?;
        self.i += n;
        Some(s)
    }

    fn cstr(&mut self) -> Option<&'a [u8]> {
        let n = self.b.get(self.i..)?.iter().position(|&c| c == 0)?;
        let s = self.take(n)?;
        self.i += 1;
        Some(s)
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    fn key(&mut self) -> Option<&'a [u8]> {
        match self.strings {
            Some(t) => t.get(self.u32()? as usize).copied(),
            None => self.cstr(),
        }
    }

    /// Reads one map through its end marker. `on_path`: every enclosing key so
    /// far matched `appinfo`, `common`; at `depth` 2 that is `appinfo/common`.
    fn map(&mut self, depth: usize, on_path: bool, found: &mut Common) -> Option<()> {
        const PATH: [&[u8]; 2] = [b"appinfo", b"common"];
        if depth > MAX_DEPTH {
            return None;
        }
        let here = on_path && depth == PATH.len();
        loop {
            let ty = *self.take(1)?.first()?;
            if ty == 8 || ty == 11 {
                return Some(());
            }
            let key = self.key()?;
            match ty {
                0 => {
                    let deeper = on_path && depth < PATH.len() && key.eq_ignore_ascii_case(PATH[depth]);
                    self.map(depth + 1, deeper, found)?;
                }
                1 => {
                    let v = self.cstr()?;
                    if here && key.eq_ignore_ascii_case(b"type") {
                        found.demo = v.eq_ignore_ascii_case(b"demo");
                    } else if here && key.eq_ignore_ascii_case(b"parent") {
                        found.parent = std::str::from_utf8(v).ok().and_then(app_id);
                    }
                }
                2 => {
                    let v = self.u32()?;
                    if here && key.eq_ignore_ascii_case(b"parent") {
                        found.parent = Some(v).filter(|&p| p != 0);
                    }
                }
                3 | 4 | 6 => {
                    self.take(4)?;
                }
                7 | 10 => {
                    self.take(8)?;
                }
                _ => return None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_id_is_digits_and_not_zero() {
        assert_eq!(app_id("4425970"), Some(4425970));
        for bad in ["default", "", "0", "+5", " 5", "5\t", "99999999999"] {
            assert_eq!(app_id(bad), None, "{bad:?}");
        }
    }

    fn environ(vars: &[&str]) -> Vec<u8> {
        vars.iter().flat_map(|v| v.bytes().chain([0])).collect()
    }

    #[test]
    fn environ_needs_a_numeric_steam_app_id() {
        assert_eq!(Launch::from_environ(&environ(&["SteamAppId=default", "HOME=/h"])), None);
        assert_eq!(Launch::from_environ(&environ(&["SteamAppIdX=5", "SteamGameId=5"])), None);
        let l = Launch::from_environ(&environ(&[
            "SteamAppId=892970",
            "STEAM_COMPAT_INSTALL_PATH=/s/steamapps/common/Guildrun Demo/",
            "STEAM_COMPAT_CLIENT_INSTALL_PATH=/s",
        ]))
        .unwrap();
        assert_eq!(l, Launch { app: 892970, install_dir: Some("guildrun demo".into()), client: Some("/s".into()) });
    }

    #[test]
    fn only_the_games_own_directory_runs_the_game() {
        let l = Launch { app: 1, install_dir: Some("valheim".into()), client: None };
        assert!(l.runs_game("/h/Steam/steamapps/common/Valheim/valheim.x86_64"));
        assert!(l.runs_game(r"S:\common\Valheim\valheim.exe"));
        let steam = "/h/Steam/steamapps/common";
        assert!(!l.runs_game(&format!("{steam}/SteamLinuxRuntime_soldier/pressure-vessel/bin/launch-client")));
        assert!(!l.runs_game(&format!("{steam}/Valheim2/valheim.x86_64")));
        assert!(!l.runs_game(r"c:\windows\system32\steam.exe"));
        assert!(!l.runs_game("/h/Steam/ubuntu12_32/reaper"));
        assert!(!Launch { install_dir: None, ..l }.runs_game(r"S:\common\Valheim\valheim.exe"));
    }

    enum Leaf<'a> {
        Str(&'a str),
        Int(u32),
    }

    /// An app ID, its `appinfo/common` leaves, its `appinfo/extended` leaves.
    type Entry<'a> = (u32, &'a [(&'a str, Leaf<'a>)], &'a [(&'a str, Leaf<'a>)]);

    /// An `appinfo.vdf` of these entries. `extended` is there to be ignored:
    /// only `common`'s `type` and `parent` count.
    fn vdf(v29: bool, apps: &[Entry]) -> Vec<u8> {
        let mut strings: Vec<String> = Vec::new();
        let mut key = |out: &mut Vec<u8>, k: &str| {
            if v29 {
                let i = strings.iter().position(|s| s == k).unwrap_or_else(|| {
                    strings.push(k.into());
                    strings.len() - 1
                });
                out.extend((i as u32).to_le_bytes());
            } else {
                out.extend(k.bytes().chain([0]));
            }
        };
        let mut body = Vec::new();
        for (app, common, extended) in apps {
            let mut kv = vec![0];
            key(&mut kv, "appinfo");
            for (name, leaves) in [("common", common), ("extended", extended)] {
                kv.push(0);
                key(&mut kv, name);
                for (k, v) in leaves.iter() {
                    match v {
                        Leaf::Str(s) => {
                            kv.push(1);
                            key(&mut kv, k);
                            kv.extend(s.bytes().chain([0]));
                        }
                        Leaf::Int(n) => {
                            kv.push(2);
                            key(&mut kv, k);
                            kv.extend(n.to_le_bytes());
                        }
                    }
                }
                kv.push(8);
            }
            kv.extend([8, 8]);
            body.extend(app.to_le_bytes());
            body.extend(((ENTRY_HEADER + kv.len()) as u32).to_le_bytes());
            body.extend([0; ENTRY_HEADER]);
            body.extend(kv);
        }
        body.extend(0u32.to_le_bytes());
        let mut out = Vec::new();
        out.extend((if v29 { V29 } else { V28 }).to_le_bytes());
        out.extend(1u32.to_le_bytes());
        if v29 {
            let table = 16 + body.len();
            out.extend((table as u64).to_le_bytes());
            out.extend(body);
            out.extend((strings.len() as u32).to_le_bytes());
            for s in &strings {
                out.extend(s.bytes().chain([0]));
            }
        } else {
            out.extend(body);
        }
        out
    }

    fn sample(v29: bool) -> Vec<u8> {
        use Leaf::*;
        vdf(
            v29,
            &[
                (4425970, &[("name", Str("Guildrun Demo")), ("type", Str("Demo")), ("parent", Int(3669200))], &[]),
                (3669200, &[("name", Str("Guildrun")), ("type", Str("Game"))], &[]),
                (7, &[("type", Str("demo")), ("parent", Str("8"))], &[]),
                (9, &[("type", Str("Game"))], &[("type", Str("Demo")), ("parent", Int(1))]),
            ],
        )
    }

    #[test]
    fn demos_map_to_their_full_game_in_both_formats() {
        let want = HashMap::from([(4425970, 3669200), (7, 8)]);
        assert_eq!(demo_parents(&sample(false)), want);
        assert_eq!(demo_parents(&sample(true)), want);
    }

    #[test]
    fn damage_keeps_what_came_before_it() {
        let file = sample(false);
        // Cut inside the second entry: the first survives, nothing panics.
        let first = 8 + 8 + le_u32(&file, 12).unwrap() as usize;
        assert_eq!(demo_parents(&file[..first + 20]), HashMap::from([(4425970, 3669200)]));
        // A v29 file cut short loses its key table, so nothing can be read.
        let file = sample(true);
        assert!(demo_parents(&file[..file.len() - 3]).is_empty());
        assert!(demo_parents(&[0x27, 0x44, 0x56, 0x07, 0, 0, 0, 0]).is_empty());
    }
}
