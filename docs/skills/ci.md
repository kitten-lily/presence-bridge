# CI

Load when changing `.github/workflows/`, or when a check fails in CI but passes
locally. Two workflows:

- `checks.yml` runs on every `pull_request`, on pushes to `main`, and on demand. It
  runs the gates AGENTS.md § Tasks and verification asks of a PR, and Renovate waits on
  it before merging ([`renovate.md`](renovate.md) § Auto-merge needs at least one
  check).
- `release.yml` runs on pushes to `main`: it tags a merged release PR and opens or
  updates the next one ([`release.md`](release.md) § How a release happens).

## Writing the workflow

- **Quote `mise run lint ::: test`.** *(here)* In a plain YAML scalar, `::: ` reads
  as a mapping key. The first push of `checks.yml` failed to parse, and GitHub only
  showed a run named after the file path, with no jobs and no log, saying "This run
  likely failed because of a workflow file issue". Any YAML parser points at the
  line: `mapping values are not allowed here`.
- **Every step is a mise task**, so a human can rerun it with `mise run <task>`.
  `mise run toolchain-check` exists for that reason (§ The Rust toolchain).
- **`permissions: contents: read`** at the top of every workflow. `checks.yml` writes
  nothing; `release.yml` writes only with a GitHub App token scoped per job
  ([`release.md`](release.md) § The token).
- **No `paths` filter.** A Renovate PR without a check never merges.
- Actions are pinned by commit SHA with the tag in a comment. Renovate's
  `github-actions` manager moves both, and the repository requires it
  (§ Repository settings).

## Repository settings

What the workflows run under, read with `gh api` on 2026-10-11, the day the repo went
public. While it was private on the free plan, rulesets weren't enforced (#4).

- **Ruleset "Default"**, on the default branch: no deletion or force push; changes only
  through a PR, merged with a merge commit, with every conversation resolved; `Checks`
  required, with the branch up to date; signed commits; CodeQL results, blocking on
  errors and on security alerts high or higher. What applies to `main`:
  `gh api repos/kitten-lily/presence-bridge/rules/branches/main -q '[.[].type]'`.
- **Ruleset "Release tags"**: `v*` tags can't be created, moved or deleted, except by
  the release App ([`release.md`](release.md) § Tags are unsigned).
- **Actions policy**: only actions created by GitHub, plus `jdx/mise-action@*`, and
  every action pinned to a full commit SHA. A new third-party action fails until it is
  added under Settings → Actions → General. Workflows from an external contributor's
  fork wait for approval, and `GITHUB_TOKEN` is read-only.
- **CodeQL default setup**, for Rust and Actions. *(here)* It kept working with SHA
  pinning required: #28's analysis passed on 2026-10-11.

*(here)* **A PR opened before the code-scanning rule existed can't merge.** #27, opened
five minutes before CodeQL was set up, showed `BLOCKED` with `Checks` green, and
`gh api 'repos/…/code-scanning/analyses?ref=refs/pull/27/merge'` returned none.
Closing and reopening it ran only `Checks`: default setup doesn't analyse a reopened
PR. A new PR does, so #27 was closed and the release PR opened again as #28
(`gh run rerun <id> --job <release-pr job>`).

## Pinning mise

`jdx/mise-action` gets an explicit `version:`, the mise that last wrote `mise.lock`.
Without one, the action keeps whatever mise the runner has, or takes the newest, so
the version drifts unseen
([krytis § Pin the mise version, not just the action](https://github.com/starlit-os/krytis/blob/main/docs/skills/ci-runner.md#pin-the-mise-version-not-just-the-action)).
Lock contents depend on which mise wrote them ([`mise.md`](mise.md) § `mise.lock`).

Renovate's own lock writer is pinned to that version with `constraints.mise`
([`renovate.md`](renovate.md) § The CI and lock-writer mise version). The same custom
manager tracks the constraint and all four action inputs across `checks.yml`,
`release.yml`, and `release-binaries.yml`, so its `jdx/mise` PR moves all five
together. A step or constraint without the annotation silently stays behind.

*(here)* Before the constraint existed, different mise writers broke Renovate PRs
in both directions:

- #17 (2026.10.3 → 2026.10.5) changed the CI pin but not the lock; the new mise
  dropped `usage`'s `repository_ids = { repository = "677629099" }` line.
- #58 and #59 changed `usage` and its lock together; Renovate added that line,
  while CI's pinned mise 2026.10.17 and 2026.10.18 respectively removed it.

The constraint prevents the second case. The first remains possible on a mise bump:
run `mise run mise-lock` with the new version pinned in the PR. If the local version
differs, download the pinned release
(`gh release download v<version> -R jdx/mise -p
'mise-v<version>-linux-x64.tar.gz'`) and run it by path in a clean environment:
`env -i HOME=$HOME PATH=<dir>/mise/bin:/usr/bin:/bin <dir>/mise/bin/mise run mise-lock`.
Putting it first on `PATH` isn't enough in a shell where mise is activated, because
`mise` there is a shell function (`type mise`).

The action adds `--locked` to `mise install` itself when `mise.lock` exists, so no
`install_args` are needed.

## The Rust toolchain

*(here)* mise selects Rust only by setting `RUSTUP_TOOLCHAIN=1.98.1` over the shared
`~/.rustup` (`mise exec -- env`). `mise.lock` records no checksum or URL for
`core:rust`, so `--locked` proves nothing about which `rustc` runs, and a runner image
ships its own stable toolchain. `mise run toolchain-check` fails unless `rustc
--version` and `cargo --version` match `mise current rust`. With
`RUSTUP_TOOLCHAIN=stable` it reported `want rustc 1.98.1, got: rustc 1.99.0` and
exited 1.

On the first run, the action installed `1.98.1` through rustup in 11 s
(`1.98.1-x86_64-unknown-linux-gnu installed`).

## Run time and timing-sensitive tests

*(here)* The first green run took 70 s on `ubuntu-26.04` with nothing cached: about
15 s for the mise action and 47 s for `lint ::: test`, which builds the release
profile (`lto = true`, `codegen-units = 1`) twice in parallel, once for clippy and once
for the tests. That isn't worth a cache action to pin and keep current.

Three IPC tests assert the 3 s session deadline inside 5–9 s limits
(`pings_do_not_extend_the_handshake_deadline`,
`a_slow_reader_does_not_extend_the_deadline_through_pong_writes`,
`pump_returns_while_the_peer_keeps_sending`). On the runner, with clippy building
alongside, the suite finished in 3.08 s. If one flakes, widen its `within` limit;
don't add retries.

## What CI doesn't prove

- **Trust.** `CI=true` makes mise trust the config by itself
  ([`mise.md`](mise.md) § New worktrees need `mise trust`), so a config that fails only
  when untrusted, for example after an `[env]` section is added, passes here.
- **Runtime behaviour.** The live check needs a logged-in Discord and a running game
  (`README.md` § Testing), so a runtime change still states it in the PR.
