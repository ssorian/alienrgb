# systemd resume restoration

The `alienrgb-resume@.service` system unit restores one user's last fully successful global profile after suspend or hibernate. It uses the systemd sleep-target lifecycle: the oneshot becomes active before `sleep.target`, remains active while the machine sleeps, and runs its `ExecStop` only when the target is torn down after resume.

The unit always runs as the instance user through `User=%i`; it must not run `alienrgb` as root. The helper performs up to 15 read-only `alienrgb doctor --json` readiness checks, one per second, then invokes exactly one live `set-all`. A failed live transport is never retried.

## State contract

A fully completed `set-all --apply` writes the selected color atomically only after all 49 transport operations succeed. The state file is:

- `$XDG_STATE_HOME/alienrgb/last-set-all-color` when `XDG_STATE_HOME` is set; or
- `$HOME/.local/state/alienrgb/last-set-all-color` otherwise.

The `alienrgb` directory is mode `0700`; the state file is atomically replaced with mode `0600`. Its complete canonical content is exactly six lowercase hexadecimal digits followed by one newline, for example `ff69b4\n`. Missing or relative state roots and write failures are explicit command failures.

The first fully successful live `set-all` creates the state. Until then, resume restoration exits successfully without issuing a write. Invalid state fails closed before hardware transport. Restoration reapplies the entire profile because reliable RGB readback is unavailable.

## Install

Build the release binary without running a live command:

```sh
cargo build --release
```

Install the fixed-path binary, helper, and unit:

```sh
sudo install -D -m 0755 target/release/alienrgb /usr/local/bin/alienrgb
sudo install -D -m 0755 contrib/systemd/alienrgb-resume /usr/local/libexec/alienrgb-resume
sudo install -D -m 0644 contrib/systemd/alienrgb-resume@.service /etc/systemd/system/alienrgb-resume@.service
sudo systemctl daemon-reload
sudo systemctl enable alienrgb-resume@"$USER".service
```

The enabled instance is `alienrgb-resume@<user>.service`. Never enable the `@root` instance; the helper also refuses UID 0 before calling `alienrgb`. The named user must already have the documented udev/ACL access to keyboard hidraw and AW-ELC USB devices. Do not add `sudo` to the helper's `alienrgb` command.

A system service normally receives the user's login home as `HOME` and therefore uses the fallback path. If the successful interactive `set-all` uses a custom absolute `XDG_STATE_HOME`, configure the same value for this unit with a per-instance systemd drop-in before enabling it; otherwise the helper intentionally looks in the HOME fallback instead.

## Verify without writing RGB

These checks do not start the unit or invoke live RGB transport:

```sh
sh -n contrib/systemd/alienrgb-resume
systemctl cat alienrgb-resume@"$USER".service
systemctl is-enabled alienrgb-resume@"$USER".service
/usr/local/bin/alienrgb doctor --json
```

Outside a sleep cycle, the enabled oneshot is normally inactive. After the first successful manually confirmed `set-all --apply`, verify the state as the same user:

```sh
state_root=${XDG_STATE_HOME:-"$HOME/.local/state"}
printf 'state file: %s\n' "$state_root/alienrgb/last-set-all-color"
wc -c < "$state_root/alienrgb/last-set-all-color"
```

The byte count must be `7`. Do not manually start or stop the service as a test: stopping an active instance intentionally runs the live restoration helper.
