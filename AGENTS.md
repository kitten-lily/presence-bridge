# AGENTS.md

Agent entry point for presence-bridge. What the service does and why is in
[`README.md`](README.md). Lessons live in `docs/skills/`; load only the file your
task needs, routed by [`docs/SKILL.md`](docs/SKILL.md).

## Tasks and verification

Cut down from [krytis § Mandatory Gates](https://github.com/starlit-os/krytis/blob/main/AGENTS.md#mandatory-gates).
How mise and the tasks behave, and the traps, are in
[`docs/skills/mise.md`](docs/skills/mise.md); this section is only the rules.

- **Task integrity.** Every maintenance action is a mise task under `mise/tasks/`: no
  loose shell commands to repeat, and no standalone scripts elsewhere. Anything an
  agent does must be repeatable by a human with `mise run <task>`. If no task covers a
  change, add one in the same change. Don't rename existing tasks without the
  maintainer's approval.
- **Toolchain.** Rust and every other tool come from `mise.toml`, pinned exactly. After
  changing a pin, run `mise run mise-lock` and commit `mise.lock` with it.
- **Verification.** A PR states that `mise run lint ::: test` passed, and links the run
  or pastes the result. `mise run lint test` is not the same command: it runs only
  `lint` (`docs/skills/mise.md` § Running several tasks). A PR that changes docs also
  states that `mise run docs-links` passed, one that changes `mise.toml` that
  `mise run mise-lock --check` did, one that changes `.github/renovate.json5` that
  `mise run renovate-check --explain` did, and one that changes `release-plz.toml`
  pastes `mise run release-preview`'s output. A change to runtime behaviour also gets
  a live check (`README.md` § Testing).
- **Disclosure.** A PR an agent wrote or helped with says so in its body: which agent
  and what it did (`CONTRIBUTING.md` § AI-assisted contributions).
- **Mise and toolchain lessons** go into `docs/skills/mise.md`, in the same commit as
  the change that taught them (§ The Self-Improvement Loop below).

## The Self-Improvement Loop

Ported from [krytis § The Self-Improvement Loop](https://github.com/starlit-os/krytis/blob/main/AGENTS.md#the-self-improvement-loop).

Every session produces two outputs:

1. **The work**: the PR, fix, or improvement.
2. **The learning**: what you discovered that the next agent should know.

Output 1 without output 2 leaves the repo no smarter. The loop only compounds if
agents write back.

```
agent works on a task
  └─ discovers a pattern / workaround / convention
       └─ writes it to the relevant docs/skills/ file
            └─ commits it in the same commit as the change
                 └─ the next agent starts smarter
```

### Same-commit rule

The skill file update is part of the implementation, not a post-task step. When you
hit a non-obvious pattern, workaround, or convention, before your next commit:

1. Open the relevant `docs/skills/` file, or create one and route it in
   [`docs/SKILL.md`](docs/SKILL.md).
2. Add the entry.
3. Stage it alongside the change that taught it.
4. Commit them together.

A lesson committed as a follow-up is a failure of the loop.

**Cross-repo exception.** Work here often comes with a krytis change (packaging, or
the sandbox facts below). One commit cannot span two repositories, so the rule is met
when **both** hold:

- the paired commits land in the same work session, and
- one cites the other's full SHA (`Pairs with kitten-lily/presence-bridge@<sha>.` or
  `Pairs with starlit-os/krytis@<sha>.`), so the pairing is traceable.

A cited SHA has to stay reachable. Merge PRs here with a **merge commit**: a squash
or rebase merge replaces the commits a krytis message cites, and so does the rebase
a stack's upper layers get when only its bottom merges
([`docs/skills/workflow.md`](docs/skills/workflow.md) § Stacked PRs). If you must rewrite a
commit another repo cites, update the citing commit in the same session. krytis
`ee15431` (on starlit-os/krytis#1152) had to be reworded after this repo's history
was rebuilt on 2026-10-09 and its cited `e016101` stopped being on any branch.

**Before opening a PR, confirm:**

- [ ] Did I discover a workaround, non-obvious pattern, or convention?
- [ ] Is the relevant `docs/skills/` file updated, in the same commit as the change?
- [ ] If no skill file covers this area, did I create one and route it?
- [ ] Do `mise run lint ::: test` and, for docs, `mise run docs-links` pass?

### Where a lesson lives

| Lesson is about | Lives in |
|---|---|
| Discord IPC protocol, client sockets and behaviour, peer handling | here, [`docs/skills/ipc.md`](docs/skills/ipc.md) |
| Detection and matching rules, the detectable list, crate dependencies, game process behaviour | here, [`docs/skills/detection.md`](docs/skills/detection.md) |
| Branches, review, merging, and cross-repo pairing in this repo | here, [`docs/skills/workflow.md`](docs/skills/workflow.md) |
| The mise toolchain, `mise.toml`, `mise.lock`, tasks | here, [`docs/skills/mise.md`](docs/skills/mise.md) |
| Renovate: `.github/renovate.json5`, which updates open and auto-merge, checking the config | here, [`docs/skills/renovate.md`](docs/skills/renovate.md) |
| Releases: `release-plz.toml`, `.github/workflows/release.yml`, what opens a release PR, the tags krytis tracks | here, [`docs/skills/release.md`](docs/skills/release.md) |
| CI: `.github/workflows/`, the mise version it installs, what it can't prove | here, [`docs/skills/ci.md`](docs/skills/ci.md) |
| Flatpak sandbox internals, BuildStream packaging, the user unit and preset in the image, freedesktop-sdk's Rust version, commit-signing hardware | krytis, [`bst.md`](https://github.com/starlit-os/krytis/blob/main/docs/skills/bst.md) and [`workflow.md`](https://github.com/starlit-os/krytis/blob/main/docs/skills/workflow.md) |

If a lesson spans both, put each half where it belongs and link them.

### What counts as a learning worth writing back

Write it:

- A workaround for an upstream bug: name the component and link the issue.
- A non-obvious pattern required for correctness.
- A convention the code does not make obvious.
- Anything you had to find by trial and error.

Don't write it:

- One-off task notes ("use commit message X for this PR").
- Things any developer already knows.
- Temporary state ("currently broken, fix pending").

### Skill files rot too: prune, don't just append

Ported from [krytis § Skill files rot too](https://github.com/starlit-os/krytis/blob/main/AGENTS.md#skill-files-rot-too--prune-dont-just-append)
and [§ how rot gets in](https://github.com/starlit-os/krytis/blob/main/AGENTS.md#what-the-first-sweep-learned-about-how-rot-gets-in-860).
When you add an entry, re-read the section you are touching and its neighbours, and
fix or cut whatever has drifted from the code. krytis found four ways docs go stale:

- **The fix gets a new section and the old one is never re-read.** When a change's
  point is "the documented thing was wrong", grep `docs/`, `README.md` and `AGENTS.md`
  for the old claim and edit it where it stands.
- **Conditional wording outlives its decision.** "Once X lands", "not yet wired up":
  when X lands or is abandoned, the sentence becomes false. Rewrite it as a plain
  statement of what is true.
- **A claim written from memory instead of checked.** Check it against the code, or
  against the system for runtime facts, before writing it. A statement about an
  inventory ("only", "all N", "nothing else") cites the command that produced it.
  Example: this repo's first README said flatpak's stub files were symlinks. `stat`
  shows empty regular files.
- **A rename rots citations in files its diff never touches.** `mise run docs-links`
  resolves `<file>.md § <heading>` citations for this reason.

Two more rules:

- **Date statements about other projects.** Any sentence about the status of an issue
  or PR elsewhere (Discord, arRPC, Proton's `rpc-bridge`, ureq, krytis) links it and
  says "as of YYYY-MM-DD". An undated status reads as current forever.
- **Open issue bodies count as documentation.** When a change makes an open issue
  wrong, edit its body in the same session:
  `gh issue list --state open --search '<term> in:body'`.

Name the owning repo when citing a file that lives elsewhere. Link it by URL, never
by a bare path, because `docs-links` would report a krytis `docs/…` path as missing
here. Keep the file name out of a `§` link's text too: `docs-links` resolves link
text like ``krytis `mise.md` § X`` against **this** repo's `mise.md`, and fails on a
heading only krytis has. Make the text `krytis § X`.

krytis runs a deliberate rot sweep with its `skill-rot-audit` skill. This repo
doesn't need one at its size; port it when `docs/skills/` passes a few hundred lines
(`wc -l docs/skills/*.md`).

### Self-diagnosis

Ported from [krytis § Self-diagnosis](https://github.com/starlit-os/krytis/blob/main/AGENTS.md#self-diagnosis).
The user may ask for a **skills check** at any time, or run `/skills-check` in Claude
Code ([`.claude/commands/skills-check.md`](.claude/commands/skills-check.md)). Run
these, report pass / fail / N/A per item with evidence, and offer to fix failures in
place.

**1. Worktree and branch naming**

- `git branch --show-current` and `git worktree list`.
- Confirm both match [`docs/skills/workflow.md`](docs/skills/workflow.md) § Worktrees and branches.

**2. Skill file updates**

Compare against the PR's actual base, not `main`. A stacked PR's base is its parent
branch, and diffing against `main` would count the parent's commits as this PR's.
Use the fetched remote ref, since a local branch of that name may be stale or absent:
`git fetch -q origin && base=origin/$(gh pr view --json baseRefName -q .baseRefName 2>/dev/null || echo main)`.

- Areas touched: `git diff "$base"...HEAD --stat -- src/ mise/ systemd/ docs/ .github/`.
- Skill file changes: `git diff "$base"...HEAD -- docs/skills/`.
- For each touched area, confirm the matching skill file was updated, or that the
  change taught nothing new. Flag gaps, and note when no skill file covers the area.

**3. Skill file commit timing**

- `git log "$base"..HEAD --oneline`, with `$base` as in item 2.
- Confirm each skill update is in the same commit as the change that produced it.
- Cross-repo work: pass when the paired commits landed in the same session and one
  cites the other's full SHA (see *Cross-repo exception* above). The unavoidable
  two-repo split is not a failure.

**4. Memory vs skill file**

- Recall the lessons written to the memory system this session.
- Confirm each has a `docs/skills/` entry here or in krytis. Memory is supplementary;
  the skill file is the authoritative record.

**5. Root-cause introspection (required for every failure)**

For each failure, answer:

- What specifically was skipped or missed?
- Why wasn't the discipline applied at the time? Pick the most accurate:
  - **Oversight**: knew the rule, had the information, didn't act on it.
  - **Misjudgement**: wrongly decided the pattern wasn't worth writing back.
  - **Rule gap**: the rule as written didn't clearly cover the case.
  - **Sequencing error**: planned to do it later; later never came.
- What would have triggered the correct behaviour?

A failure reported without a root cause is itself a compliance failure.

**Verdict:** pass / fail / N/A per item. Fix failures before the PR opens, or add a
commit to the branch if it is already open.
