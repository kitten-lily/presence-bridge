# Detection

Load when changing how games are found (`src/scan.rs`, `db::Db::matches`), the
detectable list and its cache (`db::Source`), or when checking a title live. The
matching rules themselves are in `README.md` § How it works and `src/db.rs`; this
file is what they don't say.

## Matching

- **Match Wine/Proton games on argv0, not `/proc/<pid>/exe`.** argv0 in
  `/proc/<pid>/cmdline` is the Windows path
  (`C:\Program Files (x86)\World of Warcraft\_retail_\WoW.exe`). `exe` is the Wine
  preloader and matches nothing. First recorded in starlit-os/krytis#1152 (`bst.md`).
- **Battle.net games keep their Windows path in argv0** under Faugus with
  Proton-CachyOS. Seen on 2026-10-09: `WoW.exe -launcherlogin -uid wow` and
  `Hearthstone.exe -launch -uid hs_beta`. Both matched their detectable entries
  (`_retail_/wow.exe`, `hearthstone.exe`) with no arguments rule.
- **Steam's Proton puts the game on drive `S:`, not `C:`.** Steam (flatpak) with
  Proton-CachyOS ran Valheim as `S:\common\Valheim\valheim.exe` (2026-10-09): `S:`
  is `steamapps/`. Matching is by path suffix, so the drive letter doesn't matter
  (`valheim/valheim.exe` matched), but don't key anything on `C:\`. Its siblings
  `c:\windows\system32\steam.exe` (which carries the Unix path as an argument) and
  `UnityCrashHandler64.exe` matched nothing.
- **Steam flatpak games are visible in host `/proc`, with their host path as
  argv0.** Neither the flatpak nor pressure-vessel (`SteamLinuxRuntime_soldier`)
  hides them from the host. Seen on 2026-10-09: Valheim's native build ran as
  `…/.var/app/com.valvesoftware.Steam/.local/share/Steam/steamapps/common/Valheim/valheim.x86_64`.
- **A native Linux build matches only if the list has its Linux executable**, and
  the list's entry usually describes the Proton build. Valheim's only entry is
  `valheim/valheim.exe` (`win32`), so its native build was not detected, and Discord
  showed nothing (the game doesn't use RPC either). Before expecting a native title
  to show, check its entry:
  `jq '.[] | select(.name | test("<title>"; "i"))' ~/.cache/presence-bridge/detectable.json`.
  Forcing Proton for the title in Steam makes it match the `win32` entry: Valheim
  then showed as "Playing Valheim". User-supplied aliases for the native build: #7.

## The detectable list

- **It is ~13 MB of JSON**, at `https://discord.com/api/v9/applications/detectable`.
  ureq 3's default body cap is 10 MB, so a plain `read_to_vec()` fails; `db::fetch`
  raises it with `MAX_BODY`. First recorded in starlit-os/krytis#1152 (`bst.md`).
- **ureq 3 sets no timeouts by default.** In ureq 3.4.2, `Timeouts::default()` in
  ureq's [`config.rs`](https://github.com/algesten/ureq/blob/3.4.2/src/config.rs)
  leaves `global`, `connect`, `recv_response` and `recv_body` at `None`,
  so a stalled connection never ends. `db::fetch` sets `timeout_global` per request.
  The refresh also runs on a worker thread, so even a slow fetch doesn't stop sessions
  being served (Greptile on #3).
- **Most entries can't be matched by executable.** On 2026-10-09, 14,168 of the
  24,628 apps had `"executables": []`. Guildrun is one: present, but undetectable by
  name. `db::trim` drops such entries, so check the **full** response, not the cache,
  before concluding a title is missing:
  `curl -sS https://discord.com/api/v9/applications/detectable | jq '.[] | select(.name | test("<title>"; "i"))'`.
  Every entry carries `third_party_skus`: 8,895 of the empty ones have a `steam` ID.
  Matching on those is #8.

## Steam's environment

- **Steam tags its whole launch tree with `SteamAppId`**, readable from the host in
  `/proc/<pid>/environ`. While the Guildrun Demo ran under Proton (2026-10-09), about
  20 processes carried `SteamAppId=4425970`. They included Steam's `reaper` wrapper
  (lowest PID), pressure-vessel, Wine's services, Proton's `discord\bridge.exe`, the
  game and its crash handlers. So "lowest PID with the ID" is the wrapper, not the
  game.
- **umu-launcher (Faugus) sets `SteamAppId=default`** for everything it launches,
  Battle.net included, so a non-numeric ID is not a Steam app.
- **A demo has its own app ID.** The demo was `4425970` and the full game `3669200`;
  only the full game is on Discord's list. Steam's store API links them
  (`store.steampowered.com/api/appdetails?appids=<id>` → `"fullgame": {"appid": …}`).
- **Some of the user's own `environ` files are unreadable** (`Permission denied`), so
  a scan that reads them must skip those quietly.

## Game processes

- **A game's process can outlive its window.** After quitting World of Warcraft,
  `WoW.exe` stayed alive about 20 s (2026-10-09), so the bridge cleared the activity
  that much later. That is the game, not the bridge: check `pgrep -af WoW.exe` before
  suspecting the exit path. Hearthstone's process was gone within one scan.

## Checking a title live

The full procedure, with a real game and the Discord flatpak, is `README.md`
§ Testing. Two shortcuts:

- **A stand-in process is enough to exercise detection and a real session.** Matching
  reads only argv0 and the arguments, so
  `bash -c "exec -a 'C:\Program Files (x86)\Hearthstone\Hearthstone.exe' sleep 20"`
  is detected as Hearthstone, reported to the real client, and cleared when it exits.
  It shows on the real Discord profile for those 20 s.
- **The service loop needs a running client; `--once` doesn't.** With no client
  socket, the loop only probes for one: no `/proc` scan, no fetch, and an empty cache
  dir stays empty. `--once` loads the list (fetching into an empty cache) and scans
  without checking for a client. So it tests detection with Discord closed, but not
  whether an activity reaches Discord. Checked 2026-10-09 against `main.rs`'s
  `opts.once` branch and with an empty `XDG_CACHE_HOME`.
