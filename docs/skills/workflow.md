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

When a change needs code that is still in an open PR, branch from that PR's branch
and open the new PR with `--base <parent-branch>`. Its diff then shows only its own
commits. Merge the parent first. GitHub retargets the child to `main` only when the
parent's branch is deleted, and this repo doesn't delete merged branches
(`delete_branch_on_merge: false`, as of 2026-10-09). So retarget it yourself:
`gh pr edit <child> --base main`.

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

## Merging

Merge with a **merge commit**, never squash or rebase. A krytis commit that pairs
with a commit here cites its SHA (AGENTS.md § The Self-Improvement Loop); squash and
rebase merges create new SHAs and leave that citation pointing at a commit on no
branch. All three merge methods are enabled on the repo (as of 2026-10-09), so pick
it explicitly.

## Commit signing

Commits are signed with an `ED25519-SK` key. `agent refused operation?`, either
instant or after a few seconds, and a sign that hangs are both a missed touch window,
not broken configuration. Retry in a loop as in
[krytis's lesson, "`agent refused operation` is a missed touch window"](https://github.com/starlit-os/krytis/blob/main/docs/skills/workflow.md#agent-refused-operation-is-a-missed-touch-window-not-a-broken-setup).
When a reword fails mid-rebase, the commit is not written: amend it with
`git commit --amend -F <msg>`, then `git rebase --continue`.
