# Detection

Load when changing how games are found (`src/scan.rs`, `db::Db::matches`,
`db::Db::steam`, `src/steam.rs`, `running` in `src/main.rs`), the detectable list and
its cache (`db::Source`), or when checking a title live. The matching rules
themselves are in `README.md` § How it works and the code; this file is what they
don't say.

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
- **A native Linux build matches by executable only if the list has its Linux
  executable**, and the list's entry usually describes the Proton build. Valheim's
  only executable is `valheim/valheim.exe` (`win32`), so its native build was not
  detected by name (2026-10-09), and Discord showed nothing (the game doesn't use RPC
  either). It is detected by its Steam ID instead (§ Steam's environment). Before
  expecting a native title from outside Steam to show, check its entry:
  `jq '.[] | select(.name | test("<title>"; "i"))' ~/.cache/presence-bridge/detectable-v2.json`.
  User-supplied aliases for such builds are designed in #7, closed as not planned on
  2026-10-10. Reopen it when a game needs them; its body lists the checks first.

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
- **The fetch names the project.** The endpoint isn't a documented API, so `db::fetch`
  sends `User-Agent: presence-bridge/<version> (+<repository>)`, built from
  `Cargo.toml`, instead of ureq's default `ureq/<version>`. *(here)* A `.header()` on
  the request replaces ureq 3.4.2's own: a capture on a local socket showed one
  `user-agent:` line, ours.
- **Most entries can't be matched by executable.** On 2026-10-09, 14,168 of the
  24,628 apps had `"executables": []`. Guildrun is one. Every entry carries
  `third_party_skus`, and `db::trim` keeps an app with a Steam ID even with no
  executables, so the cache has ~19.4k apps where it had ~10.5k (3.2 MB against
  1.37 MB, measured 2026-10-10). Apps with neither are still dropped, so check the
  **full** response, not the cache, before concluding a title is missing:
  `curl -sS https://discord.com/api/v9/applications/detectable | jq '.[] | select(.name | test("<title>"; "i"))'`.
- **Store IDs are dirty.** On 2026-10-10, 12 Steam IDs had a trailing tab or space,
  each repeating a clean ID of the same app (Rayman Origins `"207490\t"`), one was
  `"$6.99"`, and 18 SKUs had `"id": null`. `trim` trims, parses and dedupes them, and
  `Sku` takes both fields as `Option` so one `null` can't fail the whole list. No
  Steam ID was shared by two apps, and none of Steam's own (client 7, Steamworks
  Redist 228980, the Steam Linux Runtimes, Proton) was on the list.
- **The cache file name carries its format.** It became `detectable-v2.json` when
  Steam IDs were added: under the old name, a cache without them would have been
  trusted for up to 24 h after an upgrade. Keep the old file as an offline fallback
  (`Source`'s `legacy`) and delete it only after a fetch has written the new one.
  Deleting it up front left a first start offline with no list at all, executable
  matching included (Greptile on #20, reproduced under `unshare -rn`). Rename it
  again when the cache gains a field matching depends on.

## Steam's environment

- **Steam tags its whole launch tree with `SteamAppId`**, native and Proton games
  alike, readable from the host in `/proc/<pid>/environ`. `SteamGameId`,
  `SteamOverlayGameId` and `STEAM_COMPAT_APP_ID` carry the same value. Seen on
  2026-10-10: Valheim's native build had 4 tagged processes (`reaper`,
  pressure-vessel's `steam-runtime-launch-client`, `pv-adverb`, `valheim.x86_64`);
  the Guildrun Demo under Proton had 19 (also `python3 …/proton`, `wineserver`, Wine's
  `C:\windows\system32\*.exe`, Proton's `discord\bridge.exe`, the game, a crash
  handler and two `crashpad_handler.exe`). The Steam client process and the `bwrap`
  between launch client and `pv-adverb` carry no `SteamAppId`, and neither does
  `steamwebhelper` in its own long-lived pressure-vessel container.
- **Under flatpak Steam, the game's container isn't `reaper`'s descendant.**
  `reaper` → `steam-runtime-launch-client`, which asks `flatpak-portal` to spawn the
  container; its `bwrap` is the portal's child, and the game shares the Steam
  flatpak's PID namespace (`ppid` chain and `/proc/<pid>/ns/pid`, native Valheim,
  2026-10-10). `reaper` itself waits on every descendant: run inside the Steam
  flatpak, `./reaper SteamLaunch AppId=1 -- bash -c "setsid sleep 8 & sleep 2"`
  exited 6 s after its child, when the orphaned `sleep` did.
- **`reaper` exits with the last of its launch.** Polling every tagged process each
  0.1 s on 2026-10-10: quitting native Valheim, all 4 vanished in one poll. Quitting
  the Guildrun Demo (Proton), the game and `crashpad_handler`s went first, Proton's
  `python3`, Wine's services and `UnityCrashHandler64.exe` 0.1 s later, and `reaper`,
  the launch client, `pv-adverb` and `wineserver` in the final poll 0.1 s after that.
  Nothing outlived `reaper`, so two launches of one title don't overlap, and
  `KnownLaunch` relies on that: overlapping launches would count as one.
- **Wine rewrites argv0 after start.** Guildrun's game process first appeared with
  argv0 `…/files/lib/wine/x86_64-unix/wine`, a Unix path outside the game's
  directory, and later read `S:\common\Guildrun Demo\Guildrun.exe`. A scan in between
  sees no game yet, so `reaper` stands in: the bridge reported Guildrun at `reaper`'s
  PID first (2026-10-10).
- **The game is the process running from its own directory, not from
  `steamapps/common/`.** Every tagged process has
  `STEAM_COMPAT_INSTALL_PATH=…/steamapps/common/<dir>`, and the game's argv0 contains
  `common/<dir>/`: `…/steamapps/common/Valheim/valheim.x86_64` natively,
  `S:\common\Guildrun Demo\Guildrun.exe` under Proton. Matching
  `steamapps/common/` alone picks the launch client, which lives in
  `steamapps/common/SteamLinuxRuntime_soldier/` (or `_4`) and has a lower PID than
  the game. `<dir>` may contain spaces. The lowest PID with the ID is `reaper`, which
  starts before the game, so it only stands in until the game starts.
- **The tree outlives the game.** Wine's services, `reaper` and crash handlers can
  still be running after the game exits, and a lone `reaper` looks the same as one
  before the game started. So `running` remembers per title what stood for it during
  the current launch (`Shown`): once that has exited, the rest of the tree doesn't
  keep the title shown. The same goes for a title matched by executable: without
  this, a crash handler in the game's directory took over once `valheim.exe` exited
  (Greptile on #20). Live, a stand-in game's title was cleared on the first scan
  after it exited, while its stand-in `reaper` kept running (2026-10-10).
- **Key that memory by launch, not by "a scan saw the tree gone".** Scans pause
  while no Discord client runs, so Valheim through Proton can end and its native
  build start without any scan in between. Keyed only by title, the old
  `Executable` hid the native game for its whole launch (Greptile on #20).
  A launch is the same while any of its processes from the last scan remains
  (`KnownLaunch`), compared by PID and start ticks so a reused PID is another
  process. Keying it by its first process (`reaper`) instead would bring an exited
  game back if `reaper` exited before the last of Wine's services, with a service
  standing in (Greptile on #20, reproduced in a test). Live, `reaper` exited last
  (above).
- **Any process in the game's directory counts as the game**, so a crash handler
  there keeps a title shown as long as it runs. That is deliberate: a game may start
  through a bootstrap binary in its directory that hands over to another and exits,
  and pinning the first PID would drop the title at the hand-over. Guildrun's crash
  handlers did not outlive it: with the first build of #20, which kept a title while
  any process of its tree ran, the title cleared when Guildrun was quit (2026-10-10).
- **umu-launcher (Faugus) sets `SteamAppId=default`** for everything it launches,
  Battle.net included, so a non-numeric ID is not a Steam app.
- **A demo has its own app ID.** The demo was `4425970` and the full game `3669200`;
  only the full game is on Discord's list. Steam's own cache says which is which,
  offline: `appcache/appinfo.vdf` under `STEAM_COMPAT_CLIENT_INSTALL_PATH` (also in
  every tagged process's environment) has `appinfo/common/type = "Demo"` and
  `appinfo/common/parent = 3669200` (an int32) for the demo. On 2026-10-10 the file
  was format v29 (magic `0x07564429`: keys are indices into a string table at the
  offset in the header), and all 29 demos in it had a `parent`. `steam::Demos`
  parses it only when an ID not on the list appears and the file's mtime has
  changed; a failed read keeps the last map and is retried. The store API
  (`store.steampowered.com/api/appdetails?appids=<id>` → `"fullgame"`) says the
  same, but online and rate-limited.
- **A path from a process's environment is untrusted.** Any process of the user's,
  sandboxed flatpak apps included, can set `STEAM_COMPAT_CLIENT_INSTALL_PATH`. A FIFO
  at `appcache/appinfo.vdf` blocked `fs::read`, and with it every scan, until
  killed. `read_appinfo` opens with `O_NONBLOCK`, then rejects anything but a
  regular file under 256 MiB (Greptile on #20, reproduced with `mkfifo`).
- **Some of the user's own `environ` files are unreadable** (`Permission denied`):
  `systemd --user`, `(sd-pam)` and `ssh-agent` on 2026-10-10. The scan treats them as
  not Steam games.

## Game processes

- **A game's process can outlive its window.** After quitting World of Warcraft,
  `WoW.exe` stayed alive about 20 s (2026-10-09), so the bridge cleared the activity
  that much later. That is the game, not the bridge: check `pgrep -af WoW.exe` before
  suspecting the exit path. Hearthstone's process was gone within one scan.

## Checking a title live

The full procedure, with a real game and the Discord flatpak, is `README.md`
§ Testing. Shortcuts and a trap:

- **A stand-in process is enough to exercise detection and a real session.**
  Executable matching reads only argv0 and the arguments, so
  `bash -c "exec -a 'C:\Program Files (x86)\Hearthstone\Hearthstone.exe' sleep 20"`
  is detected as Hearthstone, reported to the real client, and cleared when it exits.
  It shows on the real Discord profile for those 20 s. A Steam match also reads the
  environment:
  `env SteamAppId=2244480 STEAM_COMPAT_INSTALL_PATH=/x/steamapps/common/Remedium bash -c "exec -a 'S:\common\Remedium\Remedium.exe' sleep 20"`
  is detected by `--once` as Remedium: Sentinels (an entry with no executables) at
  the stand-in's PID (2026-10-10). Pick an app ID whose game isn't running: a real
  game of the same title has the lower PID, and stands for it.
- **Build before a live check.** `mise run lint ::: test` doesn't rebuild
  `target/release/presence-bridge`, and a new worktree can start with a copy of the
  main checkout's `target/`: on 2026-10-10 the first `--once` in a fresh worktree,
  after a passing `lint ::: test`, ran `main`'s binary. Run `mise run build` first,
  or use `mise run dev`, which builds.
- **The service loop needs a running client; `--once` doesn't.** With no client
  socket, the loop only probes for one: no `/proc` scan, no fetch, and an empty cache
  dir stays empty. `--once` loads the list (fetching into an empty cache) and scans
  without checking for a client. So it tests detection with Discord closed, but not
  whether an activity reaches Discord. Checked 2026-10-09 against `main.rs`'s
  `opts.once` branch and with an empty `XDG_CACHE_HOME`.
