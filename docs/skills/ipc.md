# Discord IPC

Load when changing how clients are found (`ipc::discover`), the session protocol
(`ipc::Session`), or the fake-server tests in `src/ipc.rs`.

Why a flatpak client's socket lives where it does, and why flatpak overrides don't
help, is a sandbox question. That lives in krytis
[`bst.md` § Flatpak sandboxing: Discord RPC and game detection](https://github.com/starlit-os/krytis/blob/main/docs/skills/bst.md#flatpak-sandboxing-discord-rpc-and-game-detection).
This file covers what a host-side client of those sockets needs to know.

## Finding clients

- **Glob for clients; don't list app IDs.** Any real socket at
  `$XDG_RUNTIME_DIR/.flatpak/<id>/xdg-run/discord-ipc-N` or
  `$XDG_RUNTIME_DIR/app/<id>/discord-ipc-N` is a client. Discord, Vesktop, Equibop
  and Legcord then need no entries. Try `.flatpak/` before `app/`: the latter is the
  `socat` relay the Discord flatpak's wrapper creates. First recorded in
  starlit-os/krytis#1152 (`bst.md`).
- **Accept sockets only, checked without following links.**
  `fs::symlink_metadata(p)?.file_type().is_socket()`, never `p.exists()`. On
  2026-10-09 six apps that are not Discord had an empty regular file at
  `.flatpak/<id>/xdg-run/discord-ipc-0` (`stat -c '%F %n' …`). It was left by a
  system-wide override that grants `xdg-run/discord-ipc-0` to every app. The test
  fixture in `discover_keys_by_app_and_prefers_real_socket_over_relay` is such a file.
- **Skip the canonical `$XDG_RUNTIME_DIR/discord-ipc-N`** unless `--include-host`.
  Whatever listens there shares the host PID namespace and detects games itself, so
  reporting to it doubles the activity.
- **Proton's `rpc-bridge` search order** is canonical → `app/com.discordapp.Discord/`
  → `.flatpak/dev.vencord.Vesktop/xdg-run/` → `.flatpak/com.discordapp.Discord/xdg-run/`
  → the two `snap.discord*` paths. A game under Faugus talks to Discord through
  `c:\windows\system32\discord\bridge.exe --service`. That is also why an RPC-capable
  game can show twice (`README.md` § Known limitations). Found in krytis's live
  diagnosis of 2026-08-14 ([starlit-os/krytis#591](https://github.com/starlit-os/krytis/issues/591)).

## Talking to a client

- **A handshake probe is the cheapest liveness test.** Connect, send op `0`
  `{"v":1,"client_id":"…"}`, and read one frame. `op=2 {"code":4000}` means a real
  Discord answered and rejected the ID, so the transport works. arRPC answers
  `op=1 … READY` and accepts any ID, so a READY proves nothing about the ID. From
  krytis's 2026-08-14 diagnosis ([starlit-os/krytis#595](https://github.com/starlit-os/krytis/issues/595)).
- **Repeated handshakes to a live socket intermittently time out**, about 1 in 8, on
  Discord's own socket and through its relay alike. This was seen when probing with an
  invalid `client_id` (krytis, 2026-08-14). Don't conclude "the relay is broken": the
  bridge retries a failed pair on the next scan.
- **The peer is untrusted.** Any flatpak app can put a socket at its own
  `xdg-run/discord-ipc-N`, so whatever answers may not be Discord. Nothing a peer
  sends may hold the loop or grow memory without bound:
  - The handshake and each `SET_ACTIVITY` get one end-to-end deadline (`TIMEOUT`,
    3 s), passed into both `send` and `recv_blocking`. Pings are answered inside
    that, and neither pings nor trickled bytes extend it; the read and write
    timeouts are set to the time remaining before each call. A per-call deadline
    checked only after `recv_blocking` returns never fires against a peer that
    keeps pinging. Greptile found this on #3; a peer pinging every 100 ms held
    `Session::open` past 10 s.
  - **Writes need the deadline too, in small writes.** A pong echoes its ping, so
    a near-`MAX_FRAME` ping yields a pong the socket can't hold, drained as slowly
    as the peer likes. Linux applies a Unix socket's `SO_SNDTIMEO` per internal
    buffer allocation (`sock_alloc_send_pskb`), not per `write`, so one 1 MiB
    `write` kept blocking while the peer freed a buffer every 200 ms, even with the
    timeout re-armed before it. `send` writes at most `WRITE_CHUNK` (4 KiB) per
    call, so the deadline check between calls binds. Greptile's re-review on #3
    found the unbounded write; fixed in kitten-lily/presence-bridge@514c828
    (`a_slow_reader_does_not_extend_the_deadline_through_pong_writes` held
    `Session::open` past 9 s before it).
  - `pump()` handles frames as they complete and reads at most `PUMP_BUDGET` (one
    maximal frame) per scan. The original read-until-EAGAIN loop never ended against a
    peer writing faster than it read.
- **An expired socket timeout is `WouldBlock`, not `TimedOut`.** On Linux,
  `SO_RCVTIMEO` and `SO_SNDTIMEO` expiry return EAGAIN, which std maps to
  `io::ErrorKind::WouldBlock`, with the message "Resource temporarily unavailable".
  `recv_blocking` and `send` map it to `TimedOut`, so callers and the log see a
  timeout.

## Debugging traps

| Symptom | Reality |
|---|---|
| `find $XDG_RUNTIME_DIR -name 'discord-ipc-*'` finds nothing | `find` silently misses these; `ls` or `stat` the specific paths |
| grep for `discord-ipc-0` in a client binary finds nothing | the digit is appended at runtime; search for `/discord-ipc-` |

Both are from krytis's 2026-08-14 diagnosis (starlit-os/krytis#591, #595). They moved
here from krytis `bst.md` on 2026-10-09; krytis keeps the sandbox-side trap.

## Testing against a fake server

The tests in `src/ipc.rs` bind a `UnixListener` in a temp dir and play the client on a
thread with `read_frame` / `write_frame`.

- **Wrap anything that can hang in `within(limit, …)`**, so a regression fails the
  test instead of hanging the suite.
- **A flood test must know the flood has started** before it calls `pump()`. The
  first reproduction called `pump()` before the server's first write: it returned in
  5 µs and passed against the buggy code. `pump_returns_while_the_peer_keeps_sending`
  waits on a channel the server signals after its first burst.
