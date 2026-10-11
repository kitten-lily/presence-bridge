# Releases

Load when cutting or skipping a release, changing `release-plz.toml` or
`.github/workflows/release.yml`, or working out why a release PR did or didn't open.
Set up in #5. Findings marked *(here)* come from release-plz 0.3.170, run against
`main` at `ee8a17d` on 2026-10-11.

A release is a `v<version>` tag whose tree builds. krytis's element tracks those tags
with a `track: v*` glob and builds from source
([krytis § Packaging](https://github.com/starlit-os/krytis/pull/1152), in its
presence-bridge design on an open PR as of 2026-10-11), so every tag becomes a krytis
update. No binary is attached.

## Why release-plz

[release-plz](https://release-plz.dev) keeps one release PR open that bumps
`Cargo.toml` `version`, `Cargo.lock` and `CHANGELOG.md` (git-cliff, built in), and tags
when that PR merges. The crate is `publish = false`, so it runs with **`git_only =
true`**: versions come from `v*` tags, not crates.io, and `cargo publish` is skipped.

release-please was the fallback if `git_only` failed. It didn't (§ `cargo publish`
never runs), and release-plz is a mise tool with a checksum in `mise.lock`, so the
workflow runs `mise run release` and `mise run release-pr` rather than a third-party
action.

## How a release happens

`release.yml` runs on every push to `main`, in two jobs:

1. **`release`** (`mise run release`). With **`release_always = false`** it releases
   only when `HEAD` belongs to a PR whose branch starts with `release-plz-`, found
   through GitHub's "PRs associated with a commit" API. *(here)* That works on this
   repo's merge commits: on `ee8a17d` it found `[21]` and logged `skipping release:
   current commit is not from a release PR`. Otherwise it creates the `v<version>` tag
   through the API and a GitHub Release whose body is that version's changelog.
2. **`release-pr`** (`mise run release-pr`), `needs: release`, so a release PR's merge
   is tagged before the next version is worked out from the tags. It opens or updates
   the release PR, labelled `release`, on a `release-plz-*` branch. Don't change
   `pr_branch_prefix`: the prefix is how `release` recognises the PR.

A release PR stays open while other PRs merge, and is regenerated on each push to
`main`. Nothing is tagged until a human merges it.

**`release` has no concurrency group.** A group holds one running and one pending run,
and a newer pending run cancels the older one even with `cancel-in-progress: false`.
`release` acts only on its own `HEAD`, so a release PR's merge queued behind a busy run
and then displaced by the next push would never be tagged (Greptile on #22). Runs
can't collide without one: only the run for a release PR's merge acts. `release-pr`
keeps a group, where losing a pending run is harmless because the next one reads a
newer `main`. `needs: release` orders the jobs within one run, not across runs: a push
landing seconds after a release PR's merge can run `release-pr` before the tag exists.
What release-plz proposes then hasn't been seen: a release PR opened right after a
release, whose version is already tagged, is that race, and shouldn't be merged.

## Cutting a release

1. Read the release PR: the version, and the changelog in its body.
2. Checks run on it like on any PR (§ The token). Then run the live check
   (`README.md` § Testing) on the PR's head and tick both boxes in its body: the tag
   goes straight into the krytis image.
3. Merge it with a merge commit (`workflow.md` § Merging).
4. The `Release` run on `main` tags it. Check with
   `git ls-remote --tags origin 'v*'` and `gh release view v<version>`.

To skip a release, leave the PR open; later merges update it. Closing it only lasts
until the next push to `main`, which opens a new one.

## Previewing a release

`mise run release-preview` runs `release-plz update` in a throwaway worktree of
`HEAD`, with the working tree's `release-plz.toml` copied in, and prints the version
change and `CHANGELOG.md`. It reads the local tags: `git fetch --tags` first.
`mise run release --dry-run` shows whether `HEAD` would be released, which off a
release PR's merge is always `skipping release`.

## What opens a release PR

*(here)* `cargo package --list` includes **every tracked file**: `.github/`,
`docs/skills/`, `mise/`. release-plz opens a PR when a package file changes, so on its
own any commit would. **`release_commits`** narrows it to commits whose subject
matches `^(feat|fix)(\(.+\))?!?:` or `^chore\(deps\): lock file maintenance`, the ones
that change the binary krytis builds. With `v0.1.0` tagged at `d687ec9`, the only
later commit being #21's `chore(deps): update dependency jdx/mise`, it logged `no
commit matches the release_commits regex`; without the setting it proposed 0.1.1.

How Renovate's commits land (prefixes from `:semanticPrefixFixDepsChoreOthers`, in
`config:recommended`):

| Renovate update | Subject | Opens or updates the release PR |
|---|---|---|
| `cargo` dependency | `fix(deps): …` | yes |
| lock file maintenance | `chore(deps): lock file maintenance` | yes: it moves transitive crates in `Cargo.lock` |
| `mise` tool, GitHub Action, CI mise pin | `chore(deps): …` | no |

A dependency bump never releases by itself: it auto-merges into `main` and only
updates the release PR.

A fix to CI or the tooling, scoped with a mise task's name (`fix(renovate-check)`,
`fix(release)`), matches too: `release_commits` can't exclude it, because
release-plz's regex has no look-ahead (*(here)* `(?!…)` fails with `look-around,
including look-ahead and look-behind, is not supported`). The changelog drops it
(§ The changelog), so *(here)* with only such fixes since the tag, the release PR
bumps 0.1.0 → 0.1.1 and adds **no** `CHANGELOG.md` section. Leave a release PR with an
empty changelog open: the next real change fills it. Better still, type such commits
`ci` or `chore`.

## Versions

- *(here)* **No tag yet is an initial release** at `Cargo.toml`'s version: `No release
  tag found matching pattern ^v(\d+\.\d+\.\d+)$ … treated as initial release`, next
  version 0.1.0. The first release PR adds only `CHANGELOG.md`.
- The tag is `v{{ version }}`, release-plz's default for a single crate. Changing it
  breaks krytis's `track: v*` glob.
- **`features_always_increment_minor = true`**: in 0.x a `feat` is a minor bump. *(here)*
  Without it, #20's `feat` after a `v0.1.0` at `94d7602` proposed 0.1.1.
- `semver_check = false`: cargo-semver-checks needs a library target.

## The changelog

`[changelog].commit_parsers` keeps `feat` (Added), `fix(deps)` and lock file
maintenance (Dependencies), and other `fix` (Fixed), and drops everything else. *(here)*
The defaults listed two `Merge pull request` subjects and every `docs`, `ci` and
`chore` commit under "Other". Also dropped:

- review fix-ups (`fix: address Greptile's review of #20`);
- `fix` scoped with a mise task's name, or `ci`, `mise`, `renovate`, `greptile`. *(here)*
  `fix(release)` (#22) and `fix(renovate-check)` (#10) were in v0.1.0's first release
  PR. The scopes are listed in `release-plz.toml`, so **a new task's name goes there
  too** ([`mise.md`](mise.md) § Creating a task).

`mise run docs-links` skips `CHANGELOG.md`: it repeats commit subjects as they were
when released, and a reference in one that later breaks is history.

## The token

A tag or PR created with the workflow's `GITHUB_TOKEN` triggers no workflows, so the
release PR would get no Checks run, which is what review and the PR body rely on. Each
job mints a token from a **GitHub App** with `actions/create-github-app-token`, scoped
per job (`permission-contents`, `permission-pull-requests`), and valid for an hour. The
workflow's own `GITHUB_TOKEN` stays `contents: read`. The PR's author is the App's bot.

The App, created by the maintainer: no webhook, repository permissions **Contents**
and **Pull requests** read & write, installed only on this repo. Its client ID is the
repo variable `RELEASE_APP_CLIENT_ID`, its private key the secret
`RELEASE_APP_PRIVATE_KEY`. Greptile's rule that skips `renovate[bot]` also excludes
the App's bot ([`workflow.md`](workflow.md) § Review: Greptile).

Repository setting "Allow GitHub Actions to create and approve pull requests" stays
off: the App token doesn't need it.

*(here)* **`Not Found` from `create-github-app-token` means the App isn't installed on
the repo.** The first run after #22 merged failed there, with a 404 on
`GET /repos/kitten-lily/presence-bridge/installation`. That request is already signed
with the App's key, so the client ID and key were not the problem. Once the App was
installed on the repo, `gh run rerun <id>` passed and opened #23. A private App can't
be checked from outside: `gh api apps/<slug>` returns 404 for it too.

*(here)* The first release PR, #23, was authored by `presence-bridge-release[bot]`, and
its commit is `verified: true`. Checks ran on it, and Greptile didn't review it.

## Tags are unsigned

release-plz commits to the release PR through GitHub's GraphQL API, which GitHub
signs, and creates tags through the REST API, which can't sign them. krytis's
`bst source track` pins the commit (`ref: v<version>-0-g<sha>`, as on
`elements/core/bootc.bst` in krytis), not the tag's signature, and this private repo
can't require signatures anyway (rulesets return 403, [`ci.md`](ci.md) § What CI
doesn't prove).

A signed tag would need the maintainer to run `mise run release` locally after the
merge, instead of CI: when git's `tag.gpgSign` is `true` (it is on krytis hosts),
release-plz creates the tag with the git CLI and pushes it. Not done: one manual step
and a key touch per release, for a signature nothing checks.

## `cargo publish` never runs

release-plz/release-plz#2693 (open as of 2026-10-11) reports `git_only = true` with
`publish = false` still trying `cargo publish`. *(here)* With `release_always = true`
forced, `release-plz release --dry-run` listed only `creation of tag 'v0.1.0'` and
`creation of git release` as skipped. Were it to try, `cargo publish` refuses this
crate (`package.publish must be set to true`), and CI has no registry token.
