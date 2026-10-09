# AGENTS.md

Agent entry point for presence-bridge. What the service does and why is in
[`README.md`](README.md). Lessons live in `docs/skills/`; load only the file your
task needs, routed by [`docs/SKILL.md`](docs/SKILL.md).

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
or rebase merge replaces the commits a krytis message cites. If you must rewrite a
commit another repo cites, update the citing commit in the same session. krytis
`ee15431` (on starlit-os/krytis#1152) had to be reworded after this repo's history
was rebuilt on 2026-10-09 and its cited `e016101` stopped being on any branch.

**Before opening a PR, confirm:**

- [ ] Did I discover a workaround, non-obvious pattern, or convention?
- [ ] Is the relevant `docs/skills/` file updated, in the same commit as the change?
- [ ] If no skill file covers this area, did I create one and route it?
- [ ] Does `mise run docs-links` pass?

### Where a lesson lives

| Lesson is about | Lives in |
|---|---|
| Discord IPC protocol, client sockets and behaviour, peer handling | here, [`docs/skills/ipc.md`](docs/skills/ipc.md) |
| Detection and matching rules, the detectable list, crate dependencies, game process behaviour | here, [`docs/skills/detection.md`](docs/skills/detection.md) |
| Branches, review, merging, and cross-repo pairing in this repo | here, [`docs/skills/workflow.md`](docs/skills/workflow.md) |
| The mise toolchain | here, `docs/skills/` |
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
here.

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

- Areas touched: `git diff main...HEAD --stat -- src/ mise/ systemd/ docs/`.
- Skill file changes: `git diff main...HEAD -- docs/skills/`.
- For each touched area, confirm the matching skill file was updated, or that the
  change taught nothing new. Flag gaps, and note when no skill file covers the area.

**3. Skill file commit timing**

- `git log main..HEAD --oneline`.
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
