//! Discord's detectable-applications list and the matchers.
//!
//! The list is the same one Discord's own client uses for process-scan game
//! detection. Executable matching follows arRPC's `src/process/index.js`, with
//! one deliberate difference, noted in [`candidates`]. Matching a Steam game by
//! its app ID ([`Db::steam`]) goes beyond arRPC, which reads executables only.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Deserializer, Serialize};

use crate::steam;

pub const DETECTABLE_URL: &str = "https://discord.com/api/v9/applications/detectable";

/// The full API response is ~13 MB; ureq's default body cap is 10 MB.
const MAX_BODY: u64 = 64 << 20;
/// ureq sets no timeouts by default, so a stalled connection would never end.
/// The fetch runs off the main loop, so this only bounds a hung refresh.
const FETCH_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct App {
    pub id: String,
    pub name: String,
    #[serde(default, deserialize_with = "null_as_empty")]
    pub executables: Vec<Executable>,
    /// Store listings. [`trim`] keeps only Steam's, with clean numeric IDs.
    #[serde(default, deserialize_with = "null_as_empty", skip_serializing_if = "Vec::is_empty")]
    pub third_party_skus: Vec<Sku>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Sku {
    /// Optional so a `null` can't fail the whole list; none was on 2026-10-10.
    pub distributor: Option<String>,
    /// The list has `null` IDs, and Steam IDs with a trailing tab or space.
    pub id: Option<String>,
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

/// Drop everything the matchers can never use: launcher entries (arRPC skips
/// them), macOS bundle names, and store IDs that aren't a Steam app ID. An app
/// stays if it has an executable or a Steam ID. Shrinks the cache from ~13 MB
/// to ~3.2 MB.
fn trim(apps: Vec<App>) -> Vec<App> {
    apps.into_iter()
        .filter_map(|mut app| {
            app.executables.retain(|e| !e.is_launcher && e.os.as_deref() != Some("darwin") && !e.name.is_empty());
            let mut steam: Vec<u32> = app
                .third_party_skus
                .iter()
                .filter(|s| s.distributor.as_deref() == Some("steam"))
                .filter_map(|s| steam::app_id(s.id.as_deref()?.trim()))
                .collect();
            steam.sort_unstable();
            steam.dedup();
            app.third_party_skus = steam
                .into_iter()
                .map(|id| Sku { distributor: Some("steam".into()), id: Some(id.to_string()) })
                .collect();
            (!app.executables.is_empty() || !app.third_party_skus.is_empty()).then_some(app)
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
    by_steam: HashMap<u32, u32>,
}

impl Db {
    pub fn new(apps: Vec<App>) -> Db {
        let apps = trim(apps);
        let mut by_name: HashMap<String, Vec<Entry>> = HashMap::new();
        let mut by_steam: HashMap<u32, u32> = HashMap::new();
        for (ai, app) in apps.iter().enumerate() {
            for (ei, exe) in app.executables.iter().enumerate() {
                let (key, exact) = match exe.name.strip_prefix('>') {
                    Some(rest) => (rest, true),
                    None => (exe.name.as_str(), false),
                };
                by_name.entry(key.to_owned()).or_default().push(Entry { app: ai as u32, exe: ei as u32, exact });
            }
            // `trim` left only Steam IDs. No two apps shared one on 2026-10-10;
            // should that change, the first keeps it.
            for id in app.third_party_skus.iter().filter_map(|s| s.id.as_deref()?.parse().ok()) {
                by_steam.entry(id).or_insert(ai as u32);
            }
        }
        Db { apps, by_name, by_steam }
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

    /// The app whose Steam store listing is `app`.
    pub fn steam(&self, app: u32) -> Option<&App> {
        self.by_steam.get(&app).map(|&i| &self.apps[i as usize])
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
    let mut out: Vec<String> =
        (1..=parts.len()).map(|i| parts[parts.len() - i..].join("/")).filter(|s| !s.is_empty()).collect();
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
#[derive(Clone)]
pub struct Source {
    cache: std::path::PathBuf,
    /// A cache in an older format, without Steam IDs: read only when offline with
    /// no current cache, and deleted once a fetch has replaced it.
    legacy: Option<std::path::PathBuf>,
    max_age: Duration,
    /// `--db <file>`: never fetch, never write.
    fixed: bool,
}

type Fetched = Result<Vec<App>, Box<dyn std::error::Error>>;

impl Source {
    pub fn cached(cache: std::path::PathBuf, legacy: std::path::PathBuf, max_age: Duration) -> Source {
        Source { cache, legacy: Some(legacy), max_age, fixed: false }
    }

    pub fn fixed(file: std::path::PathBuf) -> Source {
        Source { cache: file, legacy: None, max_age: Duration::MAX, fixed: true }
    }

    /// Fresh cache → cache. Stale or missing → fetch, falling back to a stale
    /// cache, then the legacy one, when offline. `None` only when there is no
    /// list at all.
    pub fn load(&self) -> Option<Db> {
        self.load_with(fetch)
    }

    fn load_with(&self, fetch: impl FnOnce() -> Fetched) -> Option<Db> {
        if self.fixed {
            return read_file(&self.cache).map_err(|e| eprintln!("cannot read {}: {e}", self.cache.display())).ok();
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
                match write_file(&self.cache, &apps) {
                    Ok(()) => {
                        if let Some(legacy) = &self.legacy {
                            let _ = fs::remove_file(legacy);
                        }
                    }
                    Err(e) => eprintln!("cannot write cache {}: {e}", self.cache.display()),
                }
                eprintln!("fetched detectable list: {} apps", apps.len());
                Some(Db::new(apps))
            }
            Err(e) => {
                eprintln!("cannot fetch {DETECTABLE_URL}: {e}");
                read_file(&self.cache).ok().or_else(|| read_file(self.legacy.as_deref()?).ok())
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

fn fetch() -> Fetched {
    let body = ureq::get(DETECTABLE_URL)
        .config()
        .timeout_global(Some(FETCH_TIMEOUT))
        .build()
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

    fn app(id: &str, name: &str, executables: Vec<Executable>) -> App {
        App { id: id.into(), name: name.into(), executables, third_party_skus: Vec::new() }
    }

    fn db() -> Db {
        Db::new(vec![
            app("356875762940379136", "World of Warcraft", vec![exe("_retail_/wow.exe"), exe("_retail_/wow-64.exe")]),
            app("1", "Garry's Mod", vec![Executable { arguments: Some("-game garrysmod".into()), ..exe(">hl2.exe") }]),
            app("2", "Some Launcher", vec![Executable { is_launcher: true, ..exe("launcher.exe") }]),
            app("3", "Mac Only", vec![Executable { os: Some("darwin".into()), ..exe("mac.app") }]),
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
        let db = Db::new(vec![app("9", "Bare", vec![exe("game.exe")])]);
        assert_eq!(ids(db.matches("game.exe", &[])), ["9"]);
    }

    #[test]
    fn sixty_four_bit_marker_is_stripped_once() {
        let c = candidates("/opt/game/Game64.x64");
        assert_eq!(c[0], "game64.x64");
        assert!(c.contains(&"game.x64".to_string()));
        assert!(c.contains(&"game64".to_string()));
    }

    /// Entries shaped as the API serves them.
    fn from_api(json: &str) -> Db {
        Db::new(serde_json::from_str(json).unwrap())
    }

    #[test]
    fn apps_with_only_a_steam_id_are_kept_and_found_by_it() {
        let db = from_api(
            r#"[
            {"id": "1529337689047044147", "name": "Guildrun", "executables": [],
             "third_party_skus": [{"distributor": "steam", "id": "3669200"}]},
            {"id": "5", "name": "GOG only", "executables": null,
             "third_party_skus": [{"distributor": "gog", "id": "123"}]},
            {"id": "6", "name": "No SKUs", "executables": []}
        ]"#,
        );
        assert_eq!(db.len(), 1);
        assert_eq!(db.steam(3669200).map(|a| a.name.as_str()), Some("Guildrun"));
        assert!(db.steam(123).is_none());
    }

    #[test]
    fn steam_ids_are_cleaned_and_junk_dropped() {
        let db = from_api(
            r#"[
            {"id": "1", "name": "Rayman Origins", "executables": [],
             "third_party_skus": [{"distributor": "steam", "id": "207490"}, {"distributor": "steam", "id": "207490\t"}]},
            {"id": "2", "name": "Junk", "executables": [],
             "third_party_skus": [{"distributor": "steam", "id": "$6.99"}, {"distributor": "steam", "id": null}]}
        ]"#,
        );
        assert_eq!(db.len(), 1);
        assert_eq!(db.apps[0].third_party_skus.len(), 1);
        assert_eq!(db.steam(207490).map(|a| a.id.as_str()), Some("1"));
    }

    #[test]
    fn the_old_cache_serves_offline_until_a_fetch_replaces_it() {
        let d = crate::testutil::tempdir("legacy-cache");
        let (cache, legacy) = (d.join("detectable-v2.json"), d.join("detectable.json"));
        write_file(&legacy, &[app("9", "Bare", vec![exe("game.exe")])]).unwrap();
        let src = Source::cached(cache.clone(), legacy.clone(), Duration::from_secs(3600));

        // First start after the upgrade, offline: executable matching keeps working.
        let db = src.load_with(|| Err("offline".into())).expect("the old cache is the fallback");
        assert_eq!(ids(db.matches("game.exe", &[])), ["9"]);
        assert!(legacy.exists());

        // Online: the old cache, though fresh, is not trusted; the fetch replaces it.
        let steam = Sku { distributor: Some("steam".into()), id: Some("5".into()) };
        let fetched = vec![App { third_party_skus: vec![steam], ..app("8", "New", Vec::new()) }];
        let db = src.load_with(|| Ok(fetched)).unwrap();
        assert!(db.steam(5).is_some());
        assert!(cache.exists() && !legacy.exists());
        let _ = fs::remove_dir_all(d);
    }
}
