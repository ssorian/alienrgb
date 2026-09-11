# Optional udev access rules

`70-alienrgb.rules` grants desktop-session access only to the two confirmed Alienware m16 R2 RGB controllers:

- keyboard hidraw `0d62:d2b1`;
- AW-ELC USB `187c:0551`.

The rules use restrictive mode `0660` and `TAG+="uaccess"`. The keyboard rule additionally requires hidraw kernel naming and USB interface `00`; the AW-ELC rule requires a USB device event with the expected device class and HID `03/00/00` interface signature. They do not grant broad access to hidraw or USB devices.

udev matching is only an access-control boundary. Report-descriptor hashes, physical bus/port identity, endpoint shape, serial identity when present, and kernel-driver state are runtime checks and cannot be delegated to these rules.

This repository rule is installed byte-identically and active on the verified host. The selected `/dev/hidraw1` node is root-owned mode `0660`, tagged `seat,uaccess`, and its active POSIX ACL grants the current user effective read/write access. `alienrgb doctor` checks that effective access without opening the node. Guarded keyboard live writes were performed only on the exact Alienware m16 R2 / BIOS `1.21.0` profile: Escape logical ID `0`, F1 logical ID `1`, W logical ID `43`, and Space logical ID `106` each have an individually confirmed static-red record, and one `all` operation completed the six-frame transport with the entire visible keyboard confirmed uniformly red. These permission and readiness checks do not promise support for other profiles. Do not run `alienrgb` with `sudo`.

Installation is manual and is **not** performed by the crate:

```sh
sudo install -m 0644 contrib/udev/70-alienrgb.rules /etc/udev/rules.d/70-alienrgb.rules
sudo udevadm control --reload-rules
sudo udevadm trigger --subsystem-match=hidraw
```

Those administrative commands are documentation for a human administrator; this project does not execute them. Unplug/replug the affected controller or sign in again if the active session has not received the `uaccess` ACL. Verify the resulting access with `alienrgb doctor` rather than running the CLI as root.
