# The user service

`presence-bridge service install|uninstall` (`src/service.rs`) turns the systemd user
service on and off. What it does is in [`README.md`](../../README.md) § As a service;
this file has what the code doesn't make obvious, and how to test it.

## Telling a packaged unit from none

`systemctl --user show --property=FragmentPath --value presence-bridge.service` prints
the file systemd loads the unit from, wherever on its search path that is. For a unit
that exists nowhere it prints an empty line and **exits 0**, not an error (systemd
261.3 on krytis, 2026-10-25). `install` writes its own unit only when that comes back
empty and `$XDG_CONFIG_HOME/systemd/user/presence-bridge.service` doesn't exist, so a
packaged unit (krytis's `/usr/lib/systemd/user/`) is enabled as it is.

A unit `install` wrote starts with the `MARKER` comment line. `uninstall` deletes only
a file that starts with it. Anything else at that path is left alone, including a
file the user wrote and the `/dev/null` symlink `systemctl --user mask` puts there,
which `owner()` checks with `symlink_metadata` so it never follows the link.

## `ExecStart=` quoting

The written unit runs the binary `install` was run from (`current_exe()`). In
`ExecStart=`, `%` starts a specifier and `$` a variable, so `exec_path()` doubles both.
A path with a space has to be quoted, and inside the quotes `\` and `"` are escaped.
`systemd-analyze --user verify` accepts the result. `systemctl --user show -P ExecStart`
on `"/var/home/tester/My Apps/presence-bridge"` reports
`argv[]=/var/home/tester/My Apps/presence-bridge`, one argument.

## Testing against a real user manager

The unit tests cover rendering, quoting and `owner()`. The `systemctl` calls need a
user manager with no presence-bridge unit installed, which a krytis host isn't. A
container of the krytis image (`localhost/krytis:latest` after a krytis build) works,
with the packaged unit deleted:

```bash
podman run -d --rm --name pb-svc --systemd=always localhost/krytis:latest /usr/lib/systemd/systemd
podman exec pb-svc sh -c 'useradd -m -u 1500 tester &&
  mkdir -p /run/user/1500 && chown tester: /run/user/1500 && chmod 700 /run/user/1500 &&
  rm /usr/lib/systemd/user/presence-bridge.service'
podman exec pb-svc systemd-run --unit=tester-mgr --uid=tester -p Delegate=yes -p Type=notify \
  -E XDG_RUNTIME_DIR=/run/user/1500 -E HOME=/home/tester /usr/lib/systemd/systemd --user
podman cp target/release/presence-bridge pb-svc:/home/tester/presence-bridge
podman exec --user tester -e XDG_RUNTIME_DIR=/run/user/1500 -e HOME=/home/tester pb-svc \
  /home/tester/presence-bridge service install
```

Copy the binary in with `podman cp` after `mise run build`. Don't bind-mount
`target/release/presence-bridge`: cargo replaces the file when it rebuilds, and the
mount keeps showing the old one. `mise run lint ::: test` doesn't rebuild that binary at
all.

Starting the manager the usual way fails in that container. `loginctl enable-linger`
or `systemctl start user@1500.service` fails on `user-runtime-dir@1500.service`,
whose `systemd-user-runtime-dir` logs `Failed to resolve user '1500': Unknown user`
even though `getent passwd 1500` resolves. Running `systemd --user` straight from
`podman exec` fails with `Failed to create /init.scope control group: Permission denied`,
because it needs a delegated cgroup. `systemd-run -p Delegate=yes` gives it one, and
`Type=notify` makes the call return once the manager is up (checked 2026-10-25).

To put the krytis case back, copy the host's unit in with
`podman cp /usr/lib/systemd/user/presence-bridge.service pb-svc:/usr/lib/systemd/user/`
and run `systemctl --user daemon-reload` as `tester`. The image's
`80-presence-bridge.preset` still says `disable`, so it starts out disabled, as on a
fresh krytis install.
