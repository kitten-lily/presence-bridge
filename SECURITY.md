# Security policy

## Reporting a vulnerability

Report it privately through GitHub:
[Security → Report a vulnerability](https://github.com/kitten-lily/presence-bridge/security/advisories/new).
Please don't open a public issue for it.

Include what you saw, the release (`presence-bridge --version`) or commit, and how to
reproduce it. This is a small project maintained in spare time: expect a reply within
two weeks, and a fix in a new release if the report holds up.

## Supported versions

Only the latest release gets fixes. Releases are the `v*` tags.

## What is in scope

presence-bridge runs as your user, outside any sandbox. It:

- reads `/proc/<pid>/cmdline`, `environ` and `stat` of your processes;
- connects to Discord IPC sockets under `$XDG_RUNTIME_DIR` and sends them activity
  messages;
- fetches Discord's detectable-applications list over HTTPS and caches it under
  `~/.cache/presence-bridge`;
- reads Steam's `appinfo.vdf` when it resolves Steam games.

Anything that lets another local user, a process on the other end of one of those
sockets, or the list's server make it do more than that is in scope. So is a
dependency advisory that reaches code it runs.
