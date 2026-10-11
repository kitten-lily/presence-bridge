# presence-bridge skill router

Load only the file for your current task. Writing a lesson back:
[`AGENTS.md`](../AGENTS.md) § The Self-Improvement Loop.

| I need to… | Load |
|---|---|
| Change how Discord clients are found, or debug "no client" in `--once` | [`docs/skills/ipc.md`](skills/ipc.md) § Finding clients |
| Change the handshake / `SET_ACTIVITY` / ping handling, or bound what a socket peer can do | [`docs/skills/ipc.md`](skills/ipc.md) § Talking to a client |
| Write or fix a test against the fake IPC server in `src/ipc.rs` | [`docs/skills/ipc.md`](skills/ipc.md) § Testing against a fake server |
| Change matching, or find out why a running game isn't detected | [`docs/skills/detection.md`](skills/detection.md) § Matching |
| Change how the detectable list is fetched or cached | [`docs/skills/detection.md`](skills/detection.md) § The detectable list |
| Change Steam app ID matching, which PID stands for a Steam game, or demo resolution | [`docs/skills/detection.md`](skills/detection.md) § Steam's environment |
| Check a title live, or smoke-test without launching a game | [`docs/skills/detection.md`](skills/detection.md) § Checking a title live |
| Explain an activity that clears late after quitting a game | [`docs/skills/detection.md`](skills/detection.md) § Game processes |
| Add or change a tool pin, or bump Rust | [`docs/skills/mise.md`](skills/mise.md) § The Rust pin, § Tool declarations |
| Fix `mise.lock` drift, a backend warning, or `mise run mise-lock --check` failing | [`docs/skills/mise.md`](skills/mise.md) § `mise.lock` |
| Write or change a task under `mise/tasks/`, or run several at once | [`docs/skills/mise.md`](skills/mise.md) § Creating a task, § Running several tasks |
| Debug a task that sees flags it wasn't given, or loses an env var | [`docs/skills/mise.md`](skills/mise.md) § Inherited `usage_*`, in both directions |
| Change `.github/renovate.json5`, or check what Renovate would open and auto-merge | [`docs/skills/renovate.md`](skills/renovate.md) § Checking the config |
| Find out why a Renovate PR didn't open or didn't auto-merge | [`docs/skills/renovate.md`](skills/renovate.md) § Auto-merge needs at least one check, § Silent drops under `pinDigests` |
| Cut or skip a release, or preview the next version and changelog | [`docs/skills/release.md`](skills/release.md) § Cutting a release, § Previewing a release |
| Find out why a release PR did or didn't open, or change `release-plz.toml` | [`docs/skills/release.md`](skills/release.md) § What opens a release PR |
| Change `.github/workflows/`, or fix a check that fails only in CI | [`docs/skills/ci.md`](skills/ci.md) |
| Bump the mise version CI installs | [`docs/skills/ci.md`](skills/ci.md) § Pinning mise |
| Create a worktree/branch, stack a PR, answer Greptile, or merge | [`docs/skills/workflow.md`](skills/workflow.md) |
| Fix `agent refused operation?` when committing | [`docs/skills/workflow.md`](skills/workflow.md) § Commit signing |
| Check docs for broken references (`mise run docs-links`) | [`AGENTS.md`](../AGENTS.md) § Skill files rot too |
| Run a skills compliance check (`/skills-check`) | [`AGENTS.md`](../AGENTS.md) § Self-diagnosis |
