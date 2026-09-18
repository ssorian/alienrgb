# systemd restoration

The optional system units restore one user's last fully successful global profile with the existing `/usr/local/libexec/alienrgb-resume` helper. The helper performs up to 15 read-only `alienrgb doctor --json` readiness checks, one per second, then invokes exactly one guarded live `set-all`. A failed live transport is never retried.

- `alienrgb-boot@.service` restores before graphical login. Its `Before=display-manager.service` ordering makes the helper complete before the display manager starts.
- `alienrgb-resume@.service` becomes active before `sleep.target`, remains active while the machine sleeps, and runs its `ExecStop` when the target is torn down after resume or hibernate.

Both units run as `User=%i`, use `SupplementaryGroups=alienrgb`, and must not run `alienrgb` as root. The static `alienrgb` group provides pre-login device access through the accompanying udev rule. `WantedBy=graphical.target` is deliberately used for the boot unit: enabling a per-user instance adds it to the graphical boot transaction, where it is ordered before `display-manager.service`, without starting it on non-graphical boots.

## State contract

A fully completed `set-all --apply` writes the selected color atomically only after all 49 transport operations succeed. The state file is:

- `$XDG_STATE_HOME/alienrgb/last-set-all-color` when `XDG_STATE_HOME` is set; or
- `$HOME/.local/state/alienrgb/last-set-all-color` otherwise.

The `alienrgb` directory is mode `0700`; the state file is atomically replaced with mode `0600`. Its complete canonical content is exactly six lowercase hexadecimal digits followed by one newline, for example `ff69b4\n`. Missing or relative state roots and write failures are explicit command failures.

The first fully successful live `set-all` creates the state. Until then, either unit exits successfully without issuing a write. Invalid state fails closed before hardware transport. Restoration reapplies the entire profile because reliable RGB readback is unavailable.

## Install

Build the release binary without running a live command:

```sh
cargo build --release
```

The following commands are for a human administrator. They do not start a unit or issue an RGB write:

```sh
getent group alienrgb >/dev/null || sudo groupadd --system alienrgb
sudo usermod -aG alienrgb "$USER"
sudo install -D -m 0755 target/release/alienrgb /usr/local/bin/alienrgb
sudo install -D -m 0755 contrib/systemd/alienrgb-resume /usr/local/libexec/alienrgb-resume
sudo install -D -m 0644 contrib/systemd/alienrgb-boot@.service /etc/systemd/system/alienrgb-boot@.service
sudo install -D -m 0644 contrib/systemd/alienrgb-resume@.service /etc/systemd/system/alienrgb-resume@.service
sudo install -m 0644 contrib/udev/70-alienrgb.rules /etc/udev/rules.d/70-alienrgb.rules
sudo udevadm control --reload-rules
sudo udevadm trigger --subsystem-match=hidraw
sudo udevadm trigger --subsystem-match=usb
sudo systemctl daemon-reload
sudo systemctl enable alienrgb-boot@"$USER".service
sudo systemctl enable alienrgb-resume@"$USER".service
```

New group membership applies only to new processes, so log out and back in before relying on it interactively. At boot, systemd initializes the supplementary groups for `User=%i`, including `alienrgb`; that is why the boot unit can run before login. The named user must have the documented udev access to both controllers. Never enable an `@root` instance, and do not add `sudo` to the helper's `alienrgb` command.

A system service normally receives the user's login home as `HOME` and therefore uses the fallback state path. If a successful interactive `set-all` uses a custom absolute `XDG_STATE_HOME`, configure the same value for both unit instances with per-instance systemd drop-ins before enabling them; otherwise the helper intentionally looks in the HOME fallback instead.

## Verify without writing RGB

These checks do not start the units or invoke live RGB transport:

```sh
sh -n contrib/systemd/alienrgb-resume
systemctl cat alienrgb-boot@"$USER".service
systemctl cat alienrgb-resume@"$USER".service
systemctl is-enabled alienrgb-boot@"$USER".service
systemctl is-enabled alienrgb-resume@"$USER".service
/usr/local/bin/alienrgb doctor --json
```

Outside a sleep cycle, the resume oneshot is normally inactive. After the first successful manually confirmed `set-all --apply`, verify the state as the same user:

```sh
state_root=${XDG_STATE_HOME:-"$HOME/.local/state"}
printf 'state file: %s\n' "$state_root/alienrgb/last-set-all-color"
wc -c < "$state_root/alienrgb/last-set-all-color"
```

The byte count must be `7`. Do not manually start or stop either unit: starting the boot unit executes the live restoration helper, and stopping an active resume unit executes it through `ExecStop`.

## Rollback

### Disable one user's instances

To disable only one user's future boot and resume restoration, remove only that user's enablement symlinks. Do not stop the resume instance: stopping an active instance runs the live helper through `ExecStop`.

```sh
sudo systemctl disable alienrgb-boot@"$USER".service
sudo systemctl disable alienrgb-resume@"$USER".service
```

This does not remove the shared units, helper, rule, or another user's instances.

### Uninstall the shared integration

Before removing shared files, inventory **all** enabled instances and disable every listed boot and resume instance. Substitute each actual user from the inventory; do not proceed while any enabled instance remains.

```sh
sudo systemctl list-unit-files --state=enabled 'alienrgb-boot@*.service' 'alienrgb-resume@*.service'
sudo systemctl list-units --all 'alienrgb-boot@*.service' 'alienrgb-resume@*.service'
sudo systemctl disable alienrgb-boot@<user>.service
sudo systemctl disable alienrgb-resume@<user>.service
sudo systemctl list-unit-files --state=enabled 'alienrgb-boot@*.service' 'alienrgb-resume@*.service'
```

Repeat the two `disable` commands for every instance shown by the first inventory, then confirm that the final inventory lists none. If `systemctl list-units --all` shows an active instance, do not stop it; defer shared removal until it is inactive so rollback cannot invoke the live helper.

Only after all instances are disabled and inactive, remove the shared files and reload the managers:

```sh
sudo rm -f /etc/systemd/system/alienrgb-boot@.service
sudo rm -f /etc/systemd/system/alienrgb-resume@.service
sudo rm -f /usr/local/libexec/alienrgb-resume
sudo rm -f /etc/udev/rules.d/70-alienrgb.rules
sudo systemctl daemon-reload
sudo udevadm control --reload-rules
sudo udevadm trigger --subsystem-match=hidraw
sudo udevadm trigger --subsystem-match=usb
```

This ordering prevents invalid enablement symlinks that would otherwise reference removed shared units. If the user no longer needs pre-login access, remove that membership with `sudo gpasswd -d "$USER" alienrgb`. Keep the static group while another user or unit needs it; otherwise remove it explicitly with `sudo groupdel alienrgb`.
