# Workflow

Load when creating a branch, opening or updating a PR, answering review, or merging.

## Worktrees and branches

Each issue gets its own worktree beside the main checkout, using krytis's layout
([krytis's worktree and branch policy](https://github.com/starlit-os/krytis/blob/main/AGENTS.md#worktree--branch-policy)):

| Work | Worktree | Branch |
|---|---|---|
| Issue `#N` | `../presence-bridge.worktrees/<cc-type>/gh<N>-<slug>` | `<N>-<slug>` |
| No issue | `../presence-bridge.worktrees/<branch-name>` | a Conventional Commits name, e.g. `feat/…` |

`<cc-type>` is the Conventional Commits type that best describes the work (`feat`,
`fix`, `docs`, …). The slug is the issue title, lowercased, with runs of
non-alphanumerics turned into one hyphen.

```bash
git worktree add -b 2-port-self-learning-mandate \
  ../presence-bridge.worktrees/docs/gh2-port-self-learning-mandate <base>
```

`<base>` is `main`, or the parent branch for a stacked PR.

## Stacked PRs

When a change needs code that is still in an open PR, stack it with GitHub's
[stacked pull requests](https://docs.github.com/en/pull-requests/get-started/about-stacked-prs)
(public preview as of 2026-10-10). Each layer is still an issue branch in its own
worktree (§ Worktrees and branches), created from the branch below it. The
`gh stack` extension links them and keeps them rebased:

```bash
gh extension install github/gh-stack      # once; needs gh >= 2.90. v0.2.1 was used here
gh pr create --base <parent-branch> …      # open each layer's PR as usual
gh stack init --base main <bottom-branch> <next-branch>   # local tracking
gh stack link <bottom-pr> <next-pr>        # the stack on GitHub; additive, rerun to extend
gh stack view --short
```

`init` adopts branches that are checked out in other worktrees without switching any
checkout, and its tracking lives in the clone's common git directory, so every worktree
sees it. `link` with PR numbers pushes nothing, opens no PRs and sets the bases. Use it
instead of `gh stack submit`, which writes PR titles and bodies in its own editor, or
generates them with `--auto`. A stack takes a number from the same sequence as issues
and PRs: #10 and #12 became stack #14 on 2026-10-10.

How a stack differs from PRs that merely target each other's branches:

- **Merging needs a linear history.** Once `main` moves (any merge, Renovate's
  included), `gh stack view` marks the bottom layer ⚠ and `--json` reports
  `needsRebase: true`. Stack #14 hit this when #13 merged before it. Restore it with
  `gh stack rebase`, then `gh stack push`. That rewrites every commit in the stack, so
  each one is re-signed and needs a key touch (§ Commit signing). The merge box's
  **Rebase stack** button rebases on GitHub with no touch, but then GitHub signs the
  new commits, not your key.
- **Merge from the bottom up, with a merge commit** (§ Merging), from the merge box
  or with `gh stack merge --merge`. Merging the top PR merges the whole stack at once,
  one merge commit per PR, and keeps every commit. GitHub says the legacy merge
  endpoints can't merge a stack, so `gh pr merge` may fail on one; this hasn't been
  tried here.
- **Merging only the bottom rebases the layers above it** automatically, which gives
  their commits new SHAs. So don't cite a higher layer's SHA from krytis
  (AGENTS.md § The Self-Improvement Loop) until that layer has merged. Or merge the
  whole stack at once. The layers above retarget `main` by themselves.
- **CI** runs for every PR in the stack, but each run reads the workflow from that
  PR's own branch. #10 sat below the layer that added `checks.yml` and got no CI run.
- **A fix and its lesson go in the same commit**, not just the same layer
  (AGENTS.md § Same-commit rule). If the skill file is created only in a higher layer,
  fold the lesson into the fix's commit in the lower layer's worktree. `git commit
  --amend` reaches only the layer's tip, so use `git commit --fixup=<fix-sha>`, then
  `GIT_SEQUENCE_EDITOR=: git rebase -i --autosquash <branch below>`. Then run
  `gh stack rebase --upstack`, resolve the add/add conflict on the higher layer's copy
  of the file, and `gh stack push`. Folding rewrites the fix's SHA, so if krytis
  already cites it, update the citing commit too (AGENTS.md § The Self-Improvement
  Loop).

## Review: Greptile

Greptile reviews PRs here. Its findings arrive as inline review comments with P1/P2
badges.

- Treat each finding as a claim to verify. Read the code it points at, and reproduce
  it before fixing when you can. On #3 all three were real. Two reproduced as hangs
  past 10 s with temporary tests, and one (no fetch timeout) was worse than reported.
- Reply on each inline thread with the fixing SHA, what the reproduction showed, and
  the regression test's name:
  `gh api -X POST repos/kitten-lily/presence-bridge/pulls/<n>/comments/<id>/replies -f body=…`.
- After pushing the fixes, ask for a re-review with a PR comment that says only
  `@greptileai`: `gh pr comment <n> --body @greptileai`.

### Renovate PRs get no review

`.greptile/config.json` sets `excludeAuthors: ["renovate[bot]"]`. Greptile reviews
every new PR by default, and a dependency bump gives it little to review. Two things
about the file:

- **Greptile reads it from the PR's source branch**
  ([greptile.json reference](https://www.greptile.com/docs/code-review/greptile-json-reference)),
  and Renovate cuts its branches from `main`, so the exclusion works only once it is
  on `main`.
- **Greptile's docs disagree on whether it works in a file.** The
  [`.greptile/` reference](https://www.greptile.com/docs/code-review/greptile-config-reference)
  lists `excludeAuthors`. Its
  [troubleshooting page](https://www.greptile.com/docs/code-review/controlling-nitpickiness)
  says author exclusion is "dashboard-only, not available in greptile.json"
  (both as of 2026-10-10). So exclude `renovate[bot]` in the dashboard too. The docs
  lag the UI. As of 2026-10-10 the filters are under Settings > Code Review >
  Greptile Review Configuration > Filters, and the page says "Only pull requests
  that meet all the conditions in any rule below get automatically reviewed". So
  conditions inside a rule are ANDed and rules are ORed, and with no rules every PR
  is reviewed. One rule whose only condition excludes `renovate[bot]` skips Renovate
  and reviews everything else. A rule added later reviews whatever it matches, so it
  needs the `renovate[bot]` exclusion too, or Renovate PRs that match it get reviewed
  again. The dashboard isn't in the repo, so check it there when a Renovate PR still
  gets a review.

## Merging

Merge with a **merge commit**, never squash or rebase. A krytis commit that pairs
with a commit here cites its SHA (AGENTS.md § The Self-Improvement Loop); squash and
rebase merges create new SHAs and leave that citation pointing at a commit on no
branch. Squash and rebase merges are disabled on the repo (since 2026-10-10), and
Renovate's `automergeStrategy` is `"merge"` to match ([`renovate.md`](renovate.md) § Merging).

## Commit signing

Commits are signed with an `ED25519-SK` key. `agent refused operation?`, either
instant or after a few seconds, and a sign that hangs are both a missed touch window,
not broken configuration. Retry in a loop as in
[krytis's lesson, "`agent refused operation` is a missed touch window"](https://github.com/starlit-os/krytis/blob/main/docs/skills/workflow.md#agent-refused-operation-is-a-missed-touch-window-not-a-broken-setup).
When a reword fails mid-rebase, the commit is not written: amend it with
`git commit --amend -F <msg>`, then `git rebase --continue`.
