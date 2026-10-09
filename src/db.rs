//! Discord's detectable-applications list and the executable matcher.
//!
//! The list is the same one Discord's own client uses for process-scan game
//! detection. Matching follows arRPC's `src/process/index.js` so a title is
//! detected here exactly when arRPC (Vesktop/Equibop's built-in detection)
//! would detect it — with one deliberate difference, noted in [`candidates`].

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Deserializer, Serialize};

pub const DETECTABLE_URL: &str = "https://discord.com/api/v9/applications/detectable";

/// The full API response is ~13 MB; ureq's default body cap is 10 MB.
const MAX_BODY: u64 = 64 << 20;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct App {
    pub id: String,
    pub name: String,
    #[serde(default, deserialize_with = "null_as_empty")]
    pub executables: Vec<Executable>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Executable {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_launcher: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
}

fn null_as_empty<'de, D, T>(d: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(d)?.unwrap_or_default())
}

/// Drop everything the matcher can never use: launcher entries (arRPC skips
/// them) and macOS bundle names. Shrinks the cache from ~13 MB to ~1.5 MB.
fn trim(apps: Vec<App>) -> Vec<App> {
    apps.into_iter()
        .filter_map(|mut app| {
            app.executables
                .retain(|e| !e.is_launcher && e.os.as_deref() != Some("darwin") && !e.name.is_empty());
            (!app.executables.is_empty()).then_some(app)
        })
        .collect()
}

struct Entry {
    app: u32,
    exe: u32,
    /// `>name`: must equal the process basename, not just any path suffix.
    exact: bool,
}

pub struct Db {
    apps: Vec<App>,
    by_name: HashMap<String, Vec<Entry>>,
}

impl Db {
    pub fn new(apps: Vec<App>) -> Db {
        let apps = trim(apps);
        let mut by_name: HashMap<String, Vec<Entry>> = HashMap::new();
        for (ai, app) in apps.iter().enumerate() {
            for (ei, exe) in app.executables.iter().enumerate() {
                let (key, exact) = match exe.name.strip_prefix('>') {
                    Some(rest) => (rest, true),
                    None => (exe.name.as_str(), false),
                };
                by_name.entry(key.to_owned()).or_default().push(Entry {
                    app: ai as u32,
                    exe: ei as u32,
                    exact,
                });
            }
        }
        Db { apps, by_name }
    }

    pub fn len(&self) -> usize {
        self.apps.len()
    }

    /// Every app whose executable list matches this process. `argv0` may be a
    /// Unix path or a Wine/Proton Windows path (`C:\…\WoW.exe`).
    pub fn matches(&self, argv0: &str, args: &[String]) -> Vec<&App> {
        let cands = candidates(argv0);
        let Some(basename) = cands.first() else {
            return Vec::new();
        };
        let mut joined: Option<String> = None;
        let mut hits: Vec<u32> = Vec::new();
        for cand in &cands {
            let Some(entries) = self.by_name.get(cand) else { continue };
            for e in entries {
                if hits.contains(&e.app) || (e.exact && cand != basename) {
                    continue;
                }
                let exe = &self.apps[e.app as usize].executables[e.exe as usize];
                if let Some(want) = &exe.arguments {
                    let joined = joined.get_or_insert_with(|| args.join(" "));
                    if !joined.contains(want.as_str()) {
                        continue;
                    }
                }
                hits.push(e.app);
            }
        }
        hits.into_iter().map(|i| &self.apps[i as usize]).collect()
    }
}

/// Path suffixes to compare against executable names, basename first, then the
/// same list with arRPC's "64-bit marker" tweaks (each removes the first
/// occurrence only, as JS `String.replace` does).
///
/// arRPC drops the first path component — the empty root of `/usr/bin/x` or the
/// drive of `c:/…` — but also, by the same loop bound, produces nothing at all
/// for a bare `wow.exe`. Here a single component is kept, so a game launched by
/// bare name still matches.
pub fn candidates(argv0: &str) -> Vec<String> {
    let path = argv0.to_lowercase().replace('\\', "/");
    let parts: Vec<&str> = path.split('/').collect();
    let parts = if parts.len() > 1 { &parts[1..] } else { &parts[..] };
    let mut out: Vec<String> = (1..=parts.len())
        .map(|i| parts[parts.len() - i..].join("/"))
        .filter(|s| !s.is_empty())
        .collect();
    let base = out.len();
    for i in 0..base {
        for marker in ["64", ".x64", "x64", "_64"] {
            if out[i].contains(marker) {
                let tweaked = out[i].replacen(marker, "", 1);
                if !out.contains(&tweaked) {
                    out.push(tweaked);
                }
            }
        }
    }
    out
}

/// Where the list comes from and when to look again.
pub struct Source {
    cache: std::path::PathBuf,
    max_age: Duration,
    /// `--db <file>`: never fetch, never write.
    fixed: bool,
}

impl Source {
    pub fn cached(cache: std::path::PathBuf, max_age: Duration) -> Source {
        Source { cache, max_age, fixed: false }
    }

    pub fn fixed(file: std::path::PathBuf) -> Source {
        Source { cache: file, max_age: Duration::MAX, fixed: true }
    }

    /// Fresh cache → cache. Stale or missing → fetch, falling back to a stale
    /// cache when offline. `None` only when there is no list at all.
    pub fn load(&self) -> Option<Db> {
        if self.fixed {
            return read_file(&self.cache)
                .map_err(|e| eprintln!("cannot read {}: {e}", self.cache.display()))
                .ok();
        }
        let age = fs::metadata(&self.cache)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok());
        if let Some(age) = age
            && age < self.max_age
            && let Ok(db) = read_file(&self.cache)
        {
            return Some(db);
        }
        match fetch() {
            Ok(apps) => {
                let apps = trim(apps);
                if let Err(e) = write_file(&self.cache, &apps) {
                    eprintln!("cannot write cache {}: {e}", self.cache.display());
                }
                eprintln!("fetched detectable list: {} apps", apps.len());
                Some(Db::new(apps))
            }
            Err(e) => {
                eprintln!("cannot fetch {DETECTABLE_URL}: {e}");
                read_file(&self.cache).ok()
            }
        }
    }

    pub fn is_stale(&self) -> bool {
        !self.fixed
            && fs::metadata(&self.cache)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| SystemTime::now().duration_since(t).ok())
                .is_none_or(|age| age >= self.max_age)
    }
}

fn fetch() -> Result<Vec<App>, Box<dyn std::error::Error>> {
    let body = ureq::get(DETECTABLE_URL)
        .call()?
        .body_mut()
        .with_config()
        .limit(MAX_BODY)
        .read_to_vec()?;
    Ok(serde_json::from_slice(&body)?)
}

fn read_file(path: &Path) -> io::Result<Db> {
    let apps: Vec<App> = serde_json::from_slice(&fs::read(path)?)?;
    Ok(Db::new(apps))
}

fn write_file(path: &Path, apps: &[App]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec(apps)?)?;
    fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exe(name: &str) -> Executable {
        Executable { name: name.into(), os: Some("win32".into()), is_launcher: false, arguments: None }
    }

    fn db() -> Db {
        Db::new(vec![
            App {
                id: "356875762940379136".into(),
                name: "World of Warcraft".into(),
                executables: vec![exe("_retail_/wow.exe"), exe("_retail_/wow-64.exe")],
            },
            App {
                id: "1".into(),
                name: "Garry's Mod".into(),
                executables: vec![Executable { arguments: Some("-game garrysmod".into()), ..exe(">hl2.exe") }],
            },
            App {
                id: "2".into(),
                name: "Some Launcher".into(),
                executables: vec![Executable { is_launcher: true, ..exe("launcher.exe") }],
            },
            App { id: "3".into(), name: "Mac Only".into(), executables: vec![Executable { os: Some("darwin".into()), ..exe("mac.app") }] },
        ])
    }

    fn ids(apps: Vec<&App>) -> Vec<&str> {
        apps.into_iter().map(|a| a.id.as_str()).collect()
    }

    #[test]
    fn wine_path_matches_by_directory_suffix() {
        let db = db();
        let argv0 = r"C:\Program Files (x86)\World of Warcraft\_retail_\WoW.exe";
        assert_eq!(ids(db.matches(argv0, &[])), ["356875762940379136"]);
    }

    #[test]
    fn directory_qualified_name_needs_the_directory() {
        // `_retail_/wow.exe` must not match a bare WoW.exe from elsewhere.
        assert!(db().matches(r"C:\Games\Classic\WoW.exe", &[]).is_empty());
    }

    #[test]
    fn exact_name_ignores_deeper_suffixes_and_requires_arguments() {
        let db = db();
        let args = ["-game".to_string(), "garrysmod".to_string()];
        assert_eq!(ids(db.matches("/games/hl2/hl2.exe", &args)), ["1"]);
        assert!(db.matches("/games/hl2/hl2.exe", &["-game".into(), "tf".into()]).is_empty());
        assert!(db.matches("/games/hl2/hl2.exe", &[]).is_empty());
    }

    #[test]
    fn launchers_and_darwin_entries_never_match() {
        let db = db();
        assert!(db.matches(r"C:\x\launcher.exe", &[]).is_empty());
        assert!(db.matches("/Applications/mac.app", &[]).is_empty());
        assert_eq!(db.len(), 2);
    }

    #[test]
    fn bare_name_still_matches() {
        let db = Db::new(vec![App { id: "9".into(), name: "Bare".into(), executables: vec![exe("game.exe")] }]);
        assert_eq!(ids(db.matches("game.exe", &[])), ["9"]);
    }

    #[test]
    fn sixty_four_bit_marker_is_stripped_once() {
        let c = candidates("/opt/game/Game64.x64");
        assert_eq!(c[0], "game64.x64");
        assert!(c.contains(&"game.x64".to_string()));
        assert!(c.contains(&"game64".to_string()));
    }
}
