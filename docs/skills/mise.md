# mise

Load when changing `mise.toml`, `mise.lock`, or a task under `mise/tasks/`, or when a
`mise run` misbehaves. Most of this was learned in krytis first; each section links its
source. Lessons marked *(here)* were found in this repo, some of them correcting krytis.

## Tool declarations

Ported from [krytis § Tool declarations](https://github.com/starlit-os/krytis/blob/main/docs/skills/mise.md#tool-declarations).

- **Pin every tool to an exact version, never `latest`.** Renovate's `mise` manager
  reads the version string literally, so `"latest"` gives it nothing to compare and the
  tool drops out of updates without a sign.
- **Every binary a task calls must be in `[tools]`.** `mise run` builds the task's
  `PATH` from `[tools]` plus the base system `PATH`, and drops anything else the shell
  had, even when re-exported. A task calling an undeclared binary works only on
  machines that happen to have it. The fix is `mise use <tool>@<version>` and
  `mise run mise-lock`, never a workaround in the calling shell. From krytis
  [§ `mise run` scrubs `PATH`](https://github.com/starlit-os/krytis/blob/main/docs/skills/mise.md#mise-run-scrubs-path-down-to-declared-tools--undeclared-task-deps-fail-silently).
  `git`, `grep` and coreutils come from the base system and need no entry, and so does
  `podman`, which `renovate-check` runs Renovate under. That task also calls `gh`, only
  to borrow a token, and runs without one when `gh` is missing
  ([`renovate.md`](renovate.md) § Checking the config).
- **Tasks from outside the repo show up in `mise tasks`** *(here)*. On krytis,
  `fido2:*` tasks are declared in the system-wide `/etc/mise/config.toml`, so they are
  listed next to this repo's. They are not part of this repo.

## The Rust pin

*(here)* `rust` is pinned to `1.98.1`, and `Cargo.toml` says `rust-version = "1.98"`,
because krytis builds this crate with freedesktop-sdk's Rust, not the developer's.
krytis's freedesktop-sdk junction is `freedesktop-sdk-26.08.2`
(`elements/freedesktop-sdk.bst` in krytis), whose `elements/include/rust-source.yml`
fetches `rustc-1.98.1-src.tar.xz` (checked 2026-10-09). Code that only compiles on a
newer Rust would pass here and fail in the image build. So the pin moves when krytis
moves that junction, never ahead of it. `.github/renovate.json5` turns Renovate off
for it ([`renovate.md`](renovate.md) § The `rust` pin is held).

- `components = "clippy,rustfmt"` is a comma-separated string. The array form
  (`["clippy", "rustfmt"]`) parses too, but whether it installs the components was not
  checked.
- mise installs Rust through rustup (`backend = "core:rust"`), so `mise.lock` records
  its version, specifiers and components, but **no checksum and no URL**: the lockfile
  does not pin the toolchain's bytes.

## Release asset builder

`mise run release-assets` does not use the host Rust installation. The Rust musl
target alone is insufficient for this crate: `ring` also needs a musl C compiler, and
the first host build failed looking for `x86_64-linux-musl-gcc`. The task instead
builds natively in an exact-digest official Rust Alpine amd64 image. Keep that image's
Rust version aligned with the pin above.

The task prefers podman and falls back to Docker for GitHub's runner. Rootless podman
must use `--userns=keep-id`; `--user $(id -u):$(id -g)` maps that container identity
to a subordinate host ID and cannot write the bind-mounted `target/` directory.
Docker uses the explicit UID/GID instead. The output is always Linux x64, independent
of the host architecture.

## `mise.lock`

Ported from [krytis § `mise.lock`](https://github.com/starlit-os/krytis/blob/main/docs/skills/mise.md#miselock),
corrected where mise 2026.10.3 behaves differently.

- **Write it with `mise run mise-lock`**, and check it with `mise run mise-lock --check`.
  That runs `mise lock --platform linux-x64`, which is deterministic.
- **`mise install` creates the lockfile implicitly** *(here)*. krytis records that
  mise never does. On mise 2026.10.3, with `[settings] lockfile = true`, `mise install`
  wrote one; `mise trust`, `mise ls` and `mise exec` did not. What it records depends
  on what it resolved: a first install recorded all 7 platforms for `usage`, while an
  install of an already-present tool recorded none. Delete such a file and run
  `mise run mise-lock`.
- **An implicit lockfile can pin a tool's old backend** *(here)*. It recorded
  `aqua:jdx/usage` because that was what was already installed, while the registry now
  serves `packslip:github.com/jdx/usage`. Every later command printed
  `usage is locked to aqua:jdx/usage, but the registry now installs it from packslip:…`.
  `mise backends switch usage` moves the entry and reinstalls; the lock then gains
  `signer` and `repository_ids`, matching krytis's.
- **`mise lock --platform linux-x64` doesn't prune other platforms** *(here)*. Entries
  already in the file for other platforms stay. To drop them, delete the file and relock.
- **Any mise command that resolves tools can rewrite it in passing**, so the working
  tree heals and the committed copy doesn't. `--check` therefore compares against
  **`HEAD`**, not the index: `git diff -- mise.lock` alone passes once a stale lock's
  refresh is merely staged. It also fails when `HEAD` has no `mise.lock` at all.
  Greptile caught the index version on #9, and krytis's task has the same gap.
- **The recorded checksum and URL depend on which mise binary wrote the lock**, not
  only on the pinned version: mise releases have switched build variants (`musl` ↔
  `gnu`) for the same tool version. If `--check` fails in CI but passes locally, update
  the local mise first.
- **Lockfile revisions.** This file started at `lockfile_version = 3`, the current one
  as of 2026-10-09. An older revision makes mise warn on every command until
  `mise lock --upgrade --platform linux-x64` migrates it, which krytis had to do.

## Tasks

Every task is a file task, an executable script in `mise/tasks/`, from krytis
[§ File tasks](https://github.com/starlit-os/krytis/blob/main/docs/skills/mise.md#file-tasks).
`mise/tasks/<group>/<name>` becomes `mise run <group>:<name>`.

### Creating a task

Ported from krytis [§ Creating a new task](https://github.com/starlit-os/krytis/blob/main/docs/skills/mise.md#creating-a-new-task).

1. Create `mise/tasks/<name>` with a shebang, `#MISE description="…"`, and `#USAGE`
   lines for any flags. `chmod +x` it.
2. `mise run lint` runs `usage lint --warnings-as-errors` on it, if it has a `#USAGE`
   spec.
3. `mise run <name> --help` shows the generated help.
4. Add `<name>` to the tooling scopes in `release-plz.toml`'s `commit_parsers`, so a
   `fix(<name>)` stays out of the changelog ([`release.md`](release.md) § The changelog).

- **`#USAGE` lines are KDL, not shell.** `\$` inside `help="…"` is an invalid escape.
  mise then only *warns* and runs the task with no parsed flags, so the first
  `${usage_<flag>}` under `set -u` fails as an unbound variable. Write `$` bare, or
  leave it out. This hit presence-bridge's first `run` task while it was drafted in
  krytis.
- **`usage lint` only recognises a file as a script when it has `#USAGE` lines**
  *(here)*. On a task without any, it parses the whole file as a KDL spec and reports
  `Found invalid node name` at the shebang. A task with no flags has no spec to lint, so
  `mise/tasks/lint` checks only files that contain `#USAGE`. It walks `mise/tasks`
  recursively, because grouped tasks live in subdirectories, and a `grep` on a
  directory fails in a way `if grep -q` reads as "no spec".
- **Parsed flags arrive as `$usage_<name>`** (hyphens become underscores); the raw
  arguments stay in `$@`. `mise/tasks/dev` rebuilds the binary's command line from
  `usage_*`.

### Running several tasks

*(here)* `mise run lint test` runs **only `lint`**, with `test` as its first argument.
A task that ignores its arguments then exits 0 without the tests ever running. The form
that runs both is `mise run lint ::: test`, which runs them in parallel. So `lint` and
`build` reject any argument with a hint, and `test` passes its arguments to
`cargo test` as a filter. A new task that takes no arguments should reject them the
same way.

### Calling other tasks

Ported from krytis [§ Calling other tasks](https://github.com/starlit-os/krytis/blob/main/docs/skills/mise.md#calling-other-tasks).

- `#MISE depends=["build"]` runs a prerequisite before the script body, which is how
  `dev` builds first. mise only honours `depends` when it dispatches the task itself.
- **Calling the script directly** (`./mise/tasks/<name>`) passes the caller's `usage_*`
  variables through, but **never runs the callee's `depends`**.
- **A nested `mise run <name>`** honours `depends` but ignores the caller's `usage_*`,
  so every flag has to be passed again on its command line. It also costs ~250 ms, and
  doesn't inherit the outer run's own options (`--dry-run`, `-j`).

### Inherited `usage_*`, in both directions

Ported from krytis [§ `usage_container` is inherited](https://github.com/starlit-os/krytis/blob/main/docs/skills/mise.md#usage_container-is-inherited-by-child-processes),
[§ fallback chain clobbers](https://github.com/starlit-os/krytis/blob/main/docs/skills/mise.md#a-usage_-fallback-chain-silently-clobbers-an-inherited-env-var)
and [§ inherited `usage_*`](https://github.com/starlit-os/krytis/blob/main/docs/skills/mise.md#an-inherited-usage_-makes-the-child-think-a-flag-was-passed).

A task's `usage_*` variables reach every child process it spawns.

- **It can lose a value.** `VAR="${CLI_VAR:-${usage_var:-}}"` sets `""` when no flag
  was passed, and re-exporting that wipes out a `VAR` the task inherited. Put the
  environment variable last: `${CLI_VAR:-${usage_var:-${VAR:-}}}`.
- **It can invent a value.** A child sharing a boolean flag name with its parent sees
  the parent's `usage_<flag>=true` as if the flag had been passed to it, and there is no
  `--no-<flag>` to override it. Strip it at the call site:
  `env -u usage_<flag> ./mise/tasks/<child> …`, and pass what the child needs explicitly.

### Working directory

Tasks run from the directory holding `mise.toml`, so use relative paths
(`target/release/presence-bridge`), not `$MISE_PROJECT_ROOT`. From krytis
[§ Working directory](https://github.com/starlit-os/krytis/blob/main/docs/skills/mise.md#working-directory).

### Hidden tasks

`#MISE hide=true` only drops a task from `mise tasks`; `mise run`, `depends` and CI
still use it by name, and `mise tasks --hidden` lists it. Hide only what no human types.
From krytis [§ Hidden tasks](https://github.com/starlit-os/krytis/blob/main/docs/skills/mise.md#hidden-tasks).

### Stopping `mise run dev`

Killing the `mise` process doesn't stop the bridge: `dev` runs it as a child of
mise's task shell, and on 2026-10-10 it kept running after `kill <mise pid>`
(`ps -o pid,ppid,args` showed it, its parent still alive). A second `mise run dev`
then ran a second bridge beside it, and two bridges report every game twice. Stop
it with Ctrl-C, which reaches the whole process group, or kill the bridge itself:
`pkill -f '^target/release/presence-bridge'`.

## New worktrees need `mise trust`

Run `mise trust` once in the root of every new clone or worktree. From krytis
[§ New worktrees require `mise trust`](https://github.com/starlit-os/krytis/blob/main/docs/skills/mise.md#new-worktrees-require-mise-trust),
where every `mise run` fails until then.

*(here)* Here the requirement is only partly enforced, so a run that works proves
nothing about another machine. Checked 2026-10-09 on mise 2026.10.3, in fresh clones
and a non-interactive shell:

- **This `mise.toml`** (`[tools]` and `[settings]` only), untrusted, without `CI`:
  `mise tasks` fails with `Config files in …/mise.toml are not trusted`, while
  `mise exec`, `mise run test` and `mise run mise-lock --check` run with the pinned
  tools.
- **Adding an `[env]` section** makes every command fail until `mise trust`, as in
  krytis, whose `mise.toml` has one.
- **`CI=true` trusts automatically.** It is set in GitHub Actions and was set in the
  agent shell this was found in, so a clone there never asked.

## Environment defaults a user can override

`[env]` with `{ default = "…" }` applies only when the variable is unset or empty, so
`VAR=x mise run …` overrides it. In a mise-activated shell the default is already in
the environment, so override with `env -u VAR` or a parent config. Optional values with
no meaningful default stay as `${VAR:-}` in the script. From krytis
[§ User-overrideable environment defaults](https://github.com/starlit-os/krytis/blob/main/docs/skills/mise.md#user-overrideable-environment-defaults).

## `mise which X` and the `X` on your `PATH` can differ

`mise which` names what mise would run, not what your shell runs: a stray older copy
on `PATH` answered `--help` in krytis with a subcommand the pinned version no longer had.
Check a tool's CLI against the pinned artifact, which for a locked tool is the URL in
`mise.lock`. From krytis
[§ `mise which X`](https://github.com/starlit-os/krytis/blob/main/docs/skills/mise.md#mise-which-x-and-whatever-x-resolves-to-on-your-path-can-be-different-binaries).
Here, outside mise, `cargo` resolves to mise's shim (`~/.local/share/mise/shims/cargo`),
and under `mise exec` to `~/.cargo/bin/cargo`, rustup's proxy. `rustc --version`
under `mise exec` is what confirms the pinned `1.98.1` is the one in use.
