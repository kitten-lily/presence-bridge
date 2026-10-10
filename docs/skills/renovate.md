# Renovate

Load when changing `.github/renovate.json5`, adding a manifest or workflow Renovate
should cover, or working out why an update PR didn't open or didn't merge. Ported from
[krytis's `renovate.md`](https://github.com/starlit-os/krytis/blob/main/docs/skills/renovate.md)
and its [config](https://github.com/starlit-os/krytis/blob/main/.github/renovate.json5);
lessons marked *(here)* were found in this repo. Checked against Renovate 44.149.1.

Renovate runs as the hosted Mend Renovate app; there is no workflow for it. Install the
app only once `.github/renovate.json5` is on `main`: with no config on the default
branch, Renovate opens an onboarding PR instead.

## Coverage

| Manager | Files | Auto-merge |
|---|---|---|
| `cargo` | `Cargo.toml` (+ `Cargo.lock`) | patch/minor; majors wait for a human |
| `mise` | `mise.toml` `[tools]` (+ `mise.lock`) | digest/pin/patch/minor, except `rust`, which is off |
| `github-actions` | `.github/workflows/*.yml` | digest/pin/patch/minor |
| lock file maintenance | `Cargo.lock`, `mise.lock` | yes |

`enabledManagers` is an allowlist: a manager not listed is skipped without a word,
and so is a custom regex manager unless `"custom.regex"` is listed. `github-actions` is
listed ahead of the first workflow; until then the dry-run logs
`Manager explicitly enabled in "enabledManagers" config, but found no results`.

`cargo` and `mise` updates wait `minimumReleaseAge: "1 day"`, because they auto-merge
unattended. That gate is per release, so it doesn't reach the transitive crates that
lock file maintenance moves (§ Cargo and lock files).

## The `rust` pin is held

`rust` in `mise.toml` follows the freedesktop-sdk junction krytis builds with
([`mise.md`](mise.md) § The Rust pin), so a rule turns it off and a bump stays a manual
change. The `mise` manager extracts it with `datasource: "rust-version"` and
`skipReason: "disabled"`.

*(here)* `Cargo.toml`'s `rust-version = "1.98"` is **not** extracted: the `cargo`
manager reports `depCount: 3` (`serde`, `serde_json`, `ureq`) in the dry-run, so it
needs no rule, unlike krytis's `requires-python`
([krytis § `pep621`, not `uv`](https://github.com/starlit-os/krytis/blob/main/docs/skills/renovate.md#pep621-not-uv)).

## Cargo and lock files

*(here)* **`rangeStrategy: "update-lockfile"`**, set explicitly. It is what cargo's
`auto` resolves to (`lib/modules/manager/cargo/range.ts`: `widen` if the requirement
has a `<`, else `update-lockfile`). With `serde_json` locked back to 1.0.140, the
dry-run proposed `newValue: "1"`, `newVersion: "1.0.151"`, `isLockfileUpdate: true`
on branch `renovate/serde_json-1.x-lockfile`: `Cargo.toml` keeps its bare major and
only `Cargo.lock` moves. A new major replaces the requirement.

**Lock file maintenance** is the only thing that moves transitive crates (the rustls
stack behind the detectable-list fetch, and the rest of `Cargo.lock`). *(here)*:

- `cargo` and `mise` each contribute one update, and both land on one branch,
  `renovate/lock-file-maintenance`, so one PR refreshes both lock files.
- It runs on Renovate's default `lockFileMaintenance.schedule`,
  `before 4am on monday` (UTC unless `timezone` is set).
- For `Cargo.lock` it runs `cargo update`. Edition 2024 means resolver 3, which
  prefers releases whose `rust-version` fits ours, and CI's `cargo clippy --locked`
  on the pinned `1.98.1` fails on any that don't.
- For `mise.lock` it runs `MISE_SAFE=1 mise lock --bump`, with no `--platform`. On the
  same mise as the lock's author that changes nothing (mise 2026.10.3: `Updated 1
  platform entries`, empty diff), because it keeps the platforms already in the file.
  A diff there means Renovate's mise writes the lock differently from ours
  ([`mise.md`](mise.md) § `mise.lock`): CI's `mise run mise-lock --check` then fails
  and the PR waits for a human.
- No release-age gate applies: `cargo update` takes the newest compatible release of
  every transitive crate, however new. That is the risk of auto-merging it.

## Vulnerability alerts

`osvVulnerabilityAlerts: true`, so RustSec advisories, which OSV carries, open fix
PRs for direct crates. The dry-run logs `fetchVulnerabilities() -
osvVulnerabilityAlerts=true` per package file. GitHub's Dependabot alerts are on too
(`GET /repos/…/vulnerability-alerts` returns 204, as of 2026-10-10). They list a
vulnerable *transitive* crate from `Cargo.lock` on the Security tab, whether or not
Renovate opens a PR for it; lock file maintenance is the routine fix for those.

## Merging

- **`platformCommit: "enabled"`** makes Renovate commit through the GitHub API, which
  GitHub signs ([krytis § `platformCommit`](https://github.com/starlit-os/krytis/blob/main/docs/skills/renovate.md#platformcommit)).
  Rulesets and branch protection return 403 on this private repo on the free plan,
  so nothing here requires signatures. It keeps Renovate's commits verified, like the
  signed human ones.
- **`automergeStrategy: "merge"`** matches the only enabled merge method. Squash and
  rebase were disabled on 2026-10-10 (`allow_squash_merge` and `allow_rebase_merge`
  `false`), for the reason in [`workflow.md`](workflow.md) § Merging.
- *(here)* **GitHub auto-merge can't be turned on.** `PATCH /repos/…` with
  `allow_auto_merge=true` returned 200, applied the merge-method changes in the same
  call, and left `allow_auto_merge: false` (2026-10-10). Renovate then skips
  `platformAutomerge` (`GitHub-native automerge: not enabled in repo settings` at debug
  level) and merges the PR itself, through the API with the configured strategy, on a
  later run once the branch status is green (`lib/workers/repository/update/pr/automerge.ts`).
  So an auto-merge lands on Renovate's next run, not the moment checks pass.

## Auto-merge needs at least one check

Renovate merges only on a green branch status, and a commit with no checks at all
reads as `pending`, so the PR sits open forever, `MERGEABLE` with no error
([krytis § Auto-merge needs at least one check](https://github.com/starlit-os/krytis/blob/main/docs/skills/renovate.md#auto-merge-needs-at-least-one-check-on-the-pr)).
That holds for Renovate's own merge as much as for GitHub's: the GitHub platform code
turns zero statuses into `pending`. Until a workflow runs on `pull_request`, nothing
here auto-merges. Tell a missing check from a failing one with:

```bash
gh pr view <n> --json autoMergeRequest,statusCheckRollup
```

An empty `statusCheckRollup` is the missing check. `autoMergeRequest` stays `null`
here either way, since GitHub auto-merge is off (§ Merging).

## Checking the config

```bash
mise run renovate-check              # validate the repo config, --strict
mise run renovate-check --explain    # every rule's verdict per package and update type
mise run renovate-check --dry-run    # extract and look up against the working tree
```

`--log-level debug` adds per-dependency extraction to the dry-run. The task runs
Renovate from `ghcr.io/renovatebot/renovate:latest` under rootless podman, which needs a
`/etc/subuid` and `/etc/subgid` range for your user. It reads a token from `gh auth
token` for GitHub lookups when `gh` is installed, and runs rate-limited without one.

- **Never `npx renovate`.** On a host node newer than Renovate supports, npm falls back
  to an ancient major (37.x on node 26) that validates against an outdated schema and
  reports nothing wrong. `:latest` is deliberate: the hosted app runs the newest release.
- **`renovate-config-validator` takes no path.** Given one, it validates the file as
  *global* config and skips the repo-level checks. `--strict` also fails on migration
  warnings.
- **`--dry-run` never shows auto-merge.** It stops before branches are built. `--explain`
  calls Renovate's own `applyPackageRules()` on the real config and prints `off`,
  `hold` or `AUTO` for each name a rule matches, a fall-through `(any other package)`
  per manager, and a lock file maintenance row for managers that support it. *(here)*
  The helper is a heredoc inside `mise/tasks/renovate-check`, not a separate script,
  because AGENTS.md § Tasks and verification allows none outside `mise/tasks/`.
- **The dry-run reads the working tree**, config included, so an uncommitted edit
  takes effect. Restore simulations with `git restore`.

### Simulating a stale pin: relock too

*(here)* The `mise` manager takes the current version from `mise.lock`, not
`mise.toml`. With `usage = "6.12.0"` in `mise.toml` and `6.12.1` still locked, the
dry-run reported `currentVersion: "6.12.1"` and no update. After `mise run mise-lock`
it proposed `6.12.1` on `renovate/usage-6.x`. To fake a stale cargo dep, move the lock:
`cargo update -p <crate> --precise <old>`.

### Silent drops under `pinDigests`

Under `pinDigests: true`, a dependency whose datasource can't supply a digest is
extracted, resolved, and then dropped without a PR
([krytis § Pinning the mise version](https://github.com/starlit-os/krytis/blob/main/docs/skills/renovate.md#pinning-the-mise-version)).
If a dependency never gets an update, search the debug log for
`Could not determine new digest`. *(here)* Neither `crate` (cargo) nor
`github-releases` (`usage`) hits it: both stale-pin simulations above produced their
update.
