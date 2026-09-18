# Optional udev access rules

`70-alienrgb.rules` grants access only to the two confirmed Alienware m16 R2 RGB controllers:

- keyboard hidraw `0d62:d2b1` on USB interface `00`;
- AW-ELC USB `187c:0551` with the HID `03/00/00` interface signature.

Each strict match sets `GROUP:="alienrgb"`, restrictive mode `0660`, and `TAG+="uaccess"`. The static `alienrgb` group is required for the pre-login system service: only explicitly added group members can access these nodes before login. `TAG+="uaccess"` remains in place so active graphical sessions receive their usual seat-based ACL access. The rules do not grant broad hidraw or USB access.

udev matching is only an access-control boundary. Descriptor hashes, physical bus/port identity, endpoint shape, serial identity when present, and kernel-driver state are runtime checks and cannot be delegated to these rules. Do not run `alienrgb` with `sudo`.

## Installation

These commands are for a human administrator. This project does not create the group, change users, install rules, or reload udev.

```sh
getent group alienrgb >/dev/null || sudo groupadd --system alienrgb
sudo usermod -aG alienrgb "$USER"
sudo install -m 0644 contrib/udev/70-alienrgb.rules /etc/udev/rules.d/70-alienrgb.rules
sudo udevadm control --reload-rules
sudo udevadm trigger --subsystem-match=hidraw
sudo udevadm trigger --subsystem-match=usb
```

Group membership applies only to newly created processes. Log out and back in before relying on the group in an interactive session. The boot and resume system units use `User=%i`; systemd initializes that user's supplementary groups at boot, including `alienrgb`.

Verify resulting interactive access with `alienrgb doctor`; do not run the CLI as root.

## Rollback

### Remove one user's group membership

Removing one user from `alienrgb` does not remove the shared rule, group, helper, or another user's unit instances. First disable that user's boot and resume instances as documented in the [systemd rollback](../systemd/README.md#disable-one-users-instances), then remove only that user's membership:

```sh
sudo systemctl disable alienrgb-boot@"$USER".service
sudo systemctl disable alienrgb-resume@"$USER".service
sudo gpasswd -d "$USER" alienrgb
```

Do not stop an active resume instance: its `ExecStop` invokes the live helper.

### Remove the shared rule or group

The rule and group are shared by every boot/resume instance. Before removing either, use the conservative [systemd shared-uninstall preflight](../systemd/README.md#uninstall-the-shared-integration): inventory **all** enabled and active instances, disable every enabled instance, and defer removal while any instance is active. Do not remove the rule or group while either inventory reports an alienrgb instance.

```sh
sudo systemctl list-unit-files --state=enabled 'alienrgb-boot@*.service' 'alienrgb-resume@*.service'
sudo systemctl list-units --all 'alienrgb-boot@*.service' 'alienrgb-resume@*.service'
sudo systemctl disable alienrgb-boot@<user>.service
sudo systemctl disable alienrgb-resume@<user>.service
sudo systemctl list-unit-files --state=enabled 'alienrgb-boot@*.service' 'alienrgb-resume@*.service'
sudo systemctl list-units --all 'alienrgb-boot@*.service' 'alienrgb-resume@*.service'
```

Repeat the two `disable` commands for every user shown by the inventories. Proceed only when both final inventories list none. Then remove the shared rule and refresh permissions:

```sh
sudo rm -f /etc/udev/rules.d/70-alienrgb.rules
sudo udevadm control --reload-rules
sudo udevadm trigger --subsystem-match=hidraw
sudo udevadm trigger --subsystem-match=usb
```

Removing the rule does not remove the group. Before deleting the shared group, also confirm that no user remains a member and no other service depends on it:

```sh
getent group alienrgb
sudo groupdel alienrgb
```

Run `groupdel` only after the group entry has no member names and the dependency check is clear; otherwise retain the group.
