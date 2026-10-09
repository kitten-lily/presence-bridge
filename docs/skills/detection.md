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
- **A client has to be running.** With no client socket the loop only probes for
  one: no `/proc` scan, no fetch. A smoke run with Discord closed proves nothing, and
  an empty cache dir stays empty.
