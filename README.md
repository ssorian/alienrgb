# alienrgb

`alienrgb` is a Linux-only Rust CLI for identifying and diagnosing the RGB devices in a confirmed Alienware m16 R2 configuration.

> **Current status:** dry-run behavior remains non-writing. One separately authorized global `set-all --color FF69B4` completed all 49 operations once without retry on Alienware m16 R2 / BIOS 1.21.0 with keyboard `0d62:d2b1` and AW-ELC `187c:0551`; the user confirmed the keyboard, touchpad, rear, and power lighting visibly Hot Pink. Immediate read-only sysfs showed AC online `1`, BAT0 `Charging`, capacity `36%`, so the power observation covers only the current AC-charging state, not all six power states. The run was non-atomic, with no rollback, readback, or restore, and power persistence remains unknown. This command-level evidence does not generalize individual keyboard ID identity or change the underlying mapping-derived plan/assignment states. Separate exact static-red records remain documented below. Other colors, power states, effects, models, and BIOS versions remain unvalidated.

## Compound set-all operation

`alienrgb set-all --color <RRGGBB|#RRGGBB> --dry-run [--json]` safely builds all three canonical plans without discovery, transport, or a live executor. For example, `alienrgb set-all --color FF69B4 --dry-run` reports the exact requested pink across the complete fixed scope. The guarded live syntax is `alienrgb set-all --color <RRGGBB> --apply --experimental --confirm-live-write --confirm-power-profile-write --confirm-set-all-write [--json]`; live mode rejects a leading `#`, and every confirmation is mandatory and specific to this compound risk.

The scope cannot be widened or narrowed: all 85 keyboard targets in canonical order (6 color frames, 11 HID operations), one combined ordinary AW static plan for touchpad ID `0` plus back ID `2` (4 writes), then the equal-color power ID `4` six-state profile (34 writes). That is exactly 49 transport steps in the deliberate fixed order `keyboard -> aw_static_touchpad_back -> power_profile`. Ordinary AW control may interact with power state, so the most invasive profile is final.

Live execution is non-atomic. Both process guards are acquired before discovery and held for the full run; each stage then performs fresh production discovery, validation, and backend acquisition, and drops its backend before the next stage. A failure stops all later stages but cannot undo completed transfers. Structured reports preserve completed, failed, and not-started stage counts with `rollback=not_attempted_not_available`. `transport_attempted` becomes true as soon as any backend transfer method is invoked, including a first-transfer backend error or short transfer with zero completed steps. `transport_performed` is deliberately conservative and is true whenever any transfer was attempted or completed. Canonical, guard, discovery, and acquisition failures before the first transfer leave both fields false. There is no retry, replay, polling, readback, rollback, restore, or persistence claim. The ordinary timeout envelope is about 74 seconds plus scheduling and acquisition; power-profile persistence remains unknown.

For `set-all ... --json`, invalid arguments and missing confirmations exit with status `2` and write one standalone versioned JSON object to stderr with `overall_status=preflight_failure`, `failure_code=invalid_arguments`, and both transport fields false. Runtime preflight and partial failures also use standalone JSON stderr without an `alienrgb:` prefix; stage records distinguish attempted transfers from completed steps.

The exact canonical `set-all --color FF69B4` command has one global physical validation on Alienware m16 R2 / BIOS `1.21.0` with keyboard `0d62:d2b1` and AW-ELC `187c:0551`: keyboard `11/11`, combined touchpad/back `4/4`, and power profile `34/34`, for `49/49` operations once without retry. The user visibly confirmed keyboard, touchpad, rear, and power Hot Pink. Immediate read-only sysfs showed AC online `1`, BAT0 `Charging`, capacity `36%`; therefore the pink power observation is scoped only to the current AC-charging state and does not validate the other five power states. Execution was non-atomic, with no rollback, readback, restore, or persistence determination. The top-level report records this exact command/color evidence in both dry-run and live descriptions, while all underlying keyboard, AW static, power plans, and per-target assignments retain their existing validation states. `#ff0000` global set-all and every other color remain globally mapping-derived and unvalidated.

## AW-ELC power profile

`alienrgb power-profile --color <RRGGBB|#RRGGBB> --dry-run [--json]` builds the dedicated AlienFX API v4 `power_state_profile` for AW-ELC `187c:0551` without opening or claiming USB. The guarded live form is `alienrgb power-profile --color <RRGGBB> --apply --experimental --confirm-power-profile-write [--json]`; unlike dry-run, live mode rejects a leading `#`. Both forms require the exact Alienware m16 R2 DMI profile, BIOS `1.21.0`, and a discovered `187c:0551` controller. The one supplied color intentionally represents both AC and battery colors in this first slice.

The plan contains six states in fixed order: `0x5b` AC sleep, `0x5c` AC on, `0x5d` charging, `0x5e` battery sleep, `0x5f` battery on, and `0x60` battery critical. Their packet counts are `6/5/6/6/5/5`, followed by one final play packet, for exactly 34 direct-libusb payloads. Every payload is 33 bytes without a leading report ID and is zero-padded. The exact equal-color `#ff0000` profile has the narrow battery-on validation recorded below; other colors and the other five states remain visually unobserved. Power logical ID `4` remains mapping-derived for ordinary static/address semantics. Persistence is `unknown_power_profile_state`; restore and readback are unavailable.

The encoder is translated from MIT-licensed T-Troll/alienfx-tools commit `52713b238066d1343a492018ded546ff751cfcd4`: the Alienware m16R2 mapping in `alienfx-gui/Mappings/devices.csv:1239-1245`, `SetPowerAction` and `SetV4Action` in `AlienFX-SDK/AlienFX_SDK/AlienFX_SDK.cpp`, protocol constants in `AlienFX-SDK/AlienFX_SDK/alienfx-controls.h`, and the equal AC/battery input shape documented by `alienfx-cli`. No GPL-licensed local code was used.

Live execution accepts only typed color intent. The sealed transport regenerates `encode_equal_color`, compares the complete canonical plan, repeats fresh DMI/BIOS/USB discovery, binds acquisition to bus/port/serial, and rechecks identity, interface `00`, interrupt endpoints `0x01`/`0x81`, 33-byte packet size, and inactive kernel-driver state before dispatch. It writes each of the 34 packets exactly once with the existing 500 ms per-write timeout and aborts on the first backend error or short transfer without retrying or sending later packets. The ordinary timeout exposure is up to about 17 seconds plus scheduling and acquisition. A later failure can leave a partial profile; persistence is unknown and there is no readback or restore.

Unlike upstream `SetPowerAction`, this bounded implementation deliberately does not call `WaitForReady` or perform any status polling: upstream polling can loop and conflicts with the no-retry policy. One shared nonblocking in-process AW-ELC guard covers ordinary static and power-profile execution before discovery. Cross-process exclusion remains limited to USB acquisition/interface claiming.

Ordinary `set --device chassis` rejects `power`, `power-button`, and `all` in both dry-run and apply modes and directs callers to the dedicated power-profile command. Existing `touchpad` and `back` static-color transport behavior is unchanged; their exact red validation records do not authorize other colors.

## Temporary keyboard status capture

`alienrgb keyboard-status --experimental --confirm-live-query [--json]` is a narrow diagnostic command for capturing the keyboard protocol's raw status grammar. It performs exactly one 64-byte HID `SET_FEATURE` status-query frame (`cc93` followed by zeros) and one 64-byte `GET_FEATURE` request beginning with `cc`. It never sends reset, `color_set`, loop, or update frames, and it does not intentionally change an RGB assignment.

The command accepts successful response lengths from 1 through 64 bytes and reports only the actual returned bytes as lowercase hex. One explicitly authorized standalone capture on the exact Alienware m16 R2 / BIOS 1.21.0 / `0d62:d2b1` / pinned-descriptor profile returned exactly `cc9317112100`; no color frame was sent. This is one observed device-specific status-ready signature, not a claim about general AlienFX API v5 grammar.

This command is **not** RGB readback, does not authorize a later color write, makes no persistence claim, never retries, and remains guarded by fresh hardware/descriptor validation plus both explicit confirmation flags. Live keyboard `set` accepts readiness only when its post-reset response is exactly the same six-byte signature; `[cc,93,80,...]` remains WAITUPDATE and all other lengths or signatures fail before color. A separately authorized live set subsequently observed that exact signature after reset before continuing to the physically validated Escape static-red write.

## Safety model

Device matching is deliberately conservative:

1. DMI must identify vendor `Alienware` and product `Alienware m16 R2`.
2. USB devices must match a confirmed VID/PID.
3. The keyboard descriptor is checked for the vendor usage page `0xFF89`, usage and report ID `0xCC`, and a 63-byte payload (64-byte feature report including the report ID).
4. The confirmed descriptor SHA-256 is reported as strong evidence. A changed hash with the compatible signature is reported separately for diagnosis; live keyboard writes still require the exact confirmed hash.

Do not run `alienrgb` with `sudo`. `doctor` consumes the full DMI identity and reports whether BIOS `1.21.0` matches the confirmed live profile. It preserves its explicitly labeled Unix mode-bit estimate and separately reports effective-credential read/write checks through rustix `faccessat(..., AT_EACCESS)`. The effective check accounts for POSIX ACL decisions made by the kernel without opening the node; allowed, denied, unknown, and missing states remain distinct. Sysfs discovery canonicalizes the selected USB device/interface roots and never recurses through child symlinks.

Doctor reports keyboard and chassis technical readiness separately. Keyboard readiness requires exact supported DMI/BIOS, the pinned descriptor, and an interface-00 hidraw node with effective read/write access. Chassis readiness is pre-open evidence only: exact supported DMI/BIOS, exactly one `187c:0551`, a USB bus number, a nonempty physical port path, and interface `00` when interface identity is available. `ready_for_writes` is true only when both controller profiles are technically ready. AW-ELC opened identity, endpoints, driver state, and access are revalidated only during acquisition; no readiness boolean is physical target validation.

## Confirmed hardware

The initial profile is based on this observed system:

- DMI: `Alienware m16 R2`, vendor `Alienware`, BIOS `1.21.0`.
- Per-key keyboard: USB `0d62:d2b1`, manufacturer `DELL Technologies`, product `Keyboard`, interface `00`.
  - Descriptor SHA-256: `b552e49c3a7ed64aba7c2f1889a2563b17bf80ad270430ad4d5954237c9bb24f`.
  - Vendor feature report: usage page `0xFF89`, usage/report ID `0xCC`, 63-byte payload.
  - A report ID `0x5A` with a 16-byte payload is also present, but is not used by this slice.
- Chassis AW-ELC: USB `187c:0551`, expected AlienFX API v4. `touchpad`/upstream `haptic` logical ID `0` and `back`/`chassis` logical ID `2` each have a separate narrowly scoped static `#ff0000` validation record below. `power` logical ID `4` has a distinct narrow equal-color `#ff0000` power-profile record for battery-on/discharging only; its ordinary static/address semantics remain mapping-derived and unvalidated. All three targets remain generally mapping-derived beyond those exact records.

Upstream evidence comes from [T-Troll/alienfx-tools](https://github.com/T-Troll/alienfx-tools), whose MIT-licensed [`devices.csv`](https://github.com/T-Troll/alienfx-tools/blob/master/alienfx-gui/Mappings/devices.csv) includes `Alienware m16R2 (US)`, keyboard `0d62:d2b1`, and `187c:0551` as `m16 Tron lights`. Protocol constants and mappings were translated from that MIT source; see [`NOTICE`](NOTICE) and [`LICENSE`](LICENSE). No GPL-licensed local source was used.

For API v4 framing, T-Troll's Windows HID API uses a 34-byte caller buffer: a leading report ID `0x00` followed by the 33-byte AlienFX payload. `alienrgb` deliberately models the direct-libusb form instead: exactly those 33 on-wire payload bytes, with no prepended report-ID byte. Dry-run plans remain pure data and are not sent to hardware.

## One-time physical validation record

On an Alienware m16 R2 running BIOS `1.21.0` with AW-ELC `187c:0551`, one authorized command applied static RGB `FF0000` to `touchpad` logical ID `0`. The canonical `remove`, `start`, `set_color`, and `finish_play` steps each transferred exactly 33 bytes once, with no retry. The user visually confirmed that the touchpad turned red.

A separately authorized guarded command applied static RGB `FF0000` to `back` logical ID `2` (alias `chassis`). The same four canonical steps each transferred exactly 33 bytes once, and the user confirmed that only the rear lighting became red. There was no retry, `sudo`, readback, restore, or persistence observation.

An ordinary `set ... power` attempt completed four 33-byte writes once but produced no visible effect. It is rejected by the CLI and is not validated. A later dedicated `power-profile --color ff0000` operation completed all 34 ordered 33-byte writes once, with no retry or status polling. The user immediately confirmed the power button red with no other zone change. Immediate read-only sysfs evidence was AC online `0`, BAT0 `Discharging`, capacity `22%`, so only current battery-on/discharging visible behavior is physically observed. The six-state packet transport completed, but AC, sleep, charging, and battery-critical visual behavior remain unobserved. Persistence is unknown; no readback or restore was performed.

On the same model and BIOS with keyboard `0d62:d2b1` and the pinned descriptor hash, a first authorized attempt sent only reset/status operations and aborted before color when Linux returned a previously undocumented six-byte feature response. After a separate status-only capture established `cc9317112100` and the live gate was narrowed to that exact signature, a separately authorized second attempt completed `reset`, `query_status`, `read_status`, `set_color`, `loop`, and `update` once. The writes transferred 64 bytes each, the status read returned 6 bytes, and the user visually confirmed that only Escape logical ID `0` turned static red `FF0000`.

Three later, separately authorized one-key operations on that same exact profile applied static red to F1 logical ID `1`, W logical ID `43`, and Space logical ID `106`, with each result visually confirmed separately. Two later, separately authorized one-frame static-red operations succeeded for F2+A+Arrow Right and for F1 through F12; the user confirmed only/all requested keys red. These same-color records validate those exact one-frame requested sets, but do not disambiguate each individual logical ID within a set.

A separately authorized two-frame operation set Escape, F1-F12, Home, End, and Delete to static red. It completed seven HID operations with two 64-byte `set_color` frames, and the user confirmed the requested set red. Because every key used the same color, this validates the exact two-frame requested set and transport shape without independently disambiguating every included ID.

A separately authorized `all` operation expanded the exact 85-target catalog and completed once with no retry: six 64-byte `set_color` frames, eleven HID operations total, and the six-byte `cc9317112100` status response. The user confirmed the entire visible keyboard uniformly red. This validates full-set coverage and six-frame static-red transport, but not individual mapping identity for every logical ID.

A separately authorized global `set-all --color FF69B4` then completed the fixed keyboard, combined touchpad/back, and power-profile stages `11/11 + 4/4 + 34/34 = 49/49` once without retry. The user confirmed all four visible groups Hot Pink. Immediate read-only sysfs showed AC online `1`, BAT0 `Charging`, capacity `36%`, limiting the power observation to the current AC-charging state. The operation was non-atomic; no rollback, readback, or restore was available, and power persistence was not determined. This global same-color observation does not establish individual keyboard ID identity beyond the existing separate records.

This evidence is limited to those exact model, BIOS, controllers, targets, static color, status signature, and command profiles. It does not validate other firmware, models, targets, keys, colors, or effects. There was no `sudo`, retry, reliable color readback, persistence claim, or automatic restore.

## Build requirements

- Linux with sysfs mounted at `/sys` and procfs at `/proc`.
- A stable Rust toolchain with Cargo (Rust 2021 edition).
- System `libudev` development files for hidapi's Linux hidraw backend.
- System `libusb-1.0` development files for rusb. Neither library is vendored.

```sh
cargo build
cargo test
```

## Commands

```sh
alienrgb list [--json]
alienrgb info [--json]
alienrgb doctor [--json]
alienrgb zones [--device keyboard|chassis] [--json]
alienrgb set-all --color FF69B4 --dry-run [--json]
alienrgb set-all --color ff69b4 --apply --experimental --confirm-live-write --confirm-power-profile-write --confirm-set-all-write [--json]
alienrgb power-profile --color '#ff0000' --dry-run [--json]
alienrgb power-profile --color ff0000 --apply --experimental --confirm-power-profile-write [--json]
alienrgb set --device keyboard --target esc,f1 --color '#ff0000' --dry-run [--json]
alienrgb set --device chassis --target touchpad,back --color 00ff00 --dry-run [--json]
alienrgb set --device chassis --target back --color ff0000 --apply --experimental --confirm-live-write [--json]
alienrgb set --device keyboard --target f2,a,arrow-right --color ff0000 --apply --experimental --confirm-live-write [--json]
```

The keyboard targets are logical per-key zones behind USB controller `0d62:d2b1`. `touchpad` (alias `haptic`), `back` (alias `chassis`), and `power` are logical targets behind the single AW-ELC controller `187c:0551`; they are not three additional USB devices.

- `list` shows only known m16 R2 USB devices found after the DMI gate, their stable USB identity, and whether a hidraw interface exists.
- `info` shows DMI/BIOS identity, expected capabilities, found or missing devices, the selected descriptor source, and estimated hidraw permissions.
- `doctor` emits actionable confirmed-BIOS and per-controller technical-readiness findings. Missing udev permissions do not make the command itself fail; a true readiness value is not physical target validation.
- `zones` lists only known logical target names, IDs, aliases, USB-controller ownership, and validation state. It does not inspect hardware.
- `power-profile ... --dry-run` emits the dedicated deterministic 34-packet power-state plan without transport. Live mode requires the exact dedicated `--apply --experimental --confirm-power-profile-write` flags; generic `--confirm-live-write` is rejected.
- `set ... --dry-run` requires supported DMI, the requested USB controller, and—for keyboard plans—the exact confirmed descriptor hash. It emits a deterministic plan and performs no transport. Ordinary chassis `set` rejects power targets and `all`; use `power-profile` for the power mapping.
- AW-ELC live syntax requires all of `--apply --experimental --confirm-live-write`, exactly one known chassis target (a canonical name or alias), and one color. `all`, lists, duplicate aliases, raw IDs, and unknown targets are rejected before transport.
- Keyboard live syntax also requires `--apply --experimental --confirm-live-write`, one common color, and either 1..=75 explicit nonnumeric key names in one comma list or `all` as the sole target value. `all` expands the exact 85-target catalog, including numeric-row keys. Resolved keys are sorted by logical ID and encoded into exactly `(n+14)/15` `set_color` frames. Empty items, unknown names, explicit numeric names/raw IDs, duplicate names/aliases, mixed `all` lists, repeated `--target`, and over-catalog intents are rejected before transport.
- Human live mode writes a warning to stderr before execution. JSON mode keeps stdout valid JSON and reports only a fully completed canonical execution as success.
- `--json` provides versioned, structured output for scripting (`schema_version: 1`).

`--dry-run` and `--apply` are mutually exclusive. Confirmation flags without `--apply` are errors. Physical validation is limited to the separate exact static-red records, the dedicated equal-color power-profile `#ff0000` while battery-on/discharging, and the exact global `set-all --color FF69B4` run while AC-charging. Neither power observation validates the other power states or ordinary power static/address behavior. Same-color keyboard observations do not establish individual ID identity except for the four separately executed red keys. Other colors, effects, BIOS versions, models, and combinations remain unvalidated; treat every live invocation as dangerous and experimental.

## Static-color transport boundary

Public serialized protocol plans remain diagnostic output and never execution authority. The CLI passes only typed target/color intent into crate-private keyboard, ordinary AW-ELC, and power-profile executors. Each sealed boundary regenerates and validates its complete canonical plan immediately before fresh trusted discovery, acquisition, and ordered dispatch. It rejects mismatches in assignments, metadata, frame count/order/type/length, bytes, and zero padding. Transfers are never retried or replayed automatically.

The live routes are ordinary AW-ELC static color, the dedicated AW-ELC power profile, and experimental bounded keyboard static color. The CLI permits at most 75 explicit nonnumeric names, while the sealed keyboard live intent permits 1..=85 canonical unique known key IDs so exclusive `all` can include the ten numeric-row targets; every intent has one common color and must encode exactly `(n+14)/15` `set_color` frames. Touchpad/haptic, back/chassis, the separate one-key Escape/F1/W/Space operations, and the exact one-frame F2+A+Arrow Right and F1-F12 combinations, the exact two-frame 16-key set, and full-catalog `all` have the static-red transport validation described above. Only Escape, F1, W, and Space have individual keyboard address records; other keyboard IDs remain individually mapping-derived despite observed uniform full-keyboard red and global Hot Pink coverage. The power profile has the exact equal-color `#ff0000` battery-on/discharging observation plus the global Hot Pink current AC-charging observation; its other power states and ordinary static/address behavior remain unvalidated. The global Hot Pink record applies only to the top-level canonical set-all command/color and does not upgrade underlying plan or assignment validation. Other target combinations, colors, effects, and profiles remain live-write-unvalidated. No route provides reliable color readback, automatic restore, or a persistence claim.

Fresh execution-time discovery requires the confirmed BIOS `1.21.0` for all live transports, binds keyboard selection to one exact interface-00 physical hidraw path and exact descriptor hash, and binds AW-ELC to its USB bus plus physical port path and serial when available. An AW-ELC BIOS mismatch or missing BIOS fails before factory acquisition, USB open, claim, or transfer. hidapi 2.6.7 may enumerate several collection records for that one path; the keyboard backend requires all same-path rows to agree on USB identity and requires exactly one `0xFF89`/`0x00CC` RGB collection, while ignoring unrelated paths. It then opens the selected physical path once, rechecks opened VID/PID/path/interface, and retrieves the report descriptor from the handle for an exact SHA-256 check before feature writes. It keeps `usbhid`/`hid-generic` attached, never uses libusb for the keyboard, and performs only exact 64-byte feature operations. AW-ELC behavior is unchanged.

### Linux keyboard timeout boundary

The verified host runs kernel `7.2.4-zen2-1-zen` with `CONFIG_HIDRAW=y` and `CONFIG_USB_HID=y`. Linux stable tag `v7.2.4` routes `HIDIOCSFEATURE`/`HIDIOCGFEATURE` through [`drivers/hid/hidraw.c`](https://git.kernel.org/pub/scm/linux/kernel/git/stable/linux.git/tree/drivers/hid/hidraw.c?h=v7.2.4) into usbhid raw requests in [`drivers/hid/usbhid/hid-core.c`](https://git.kernel.org/pub/scm/linux/kernel/git/stable/linux.git/tree/drivers/hid/usbhid/hid-core.c?h=v7.2.4), where `usb_control_msg(..., USB_CTRL_SET_TIMEOUT)` uses the 5000 ms constant defined by [`drivers/hid/usbhid/usbhid.h`](https://git.kernel.org/pub/scm/linux/kernel/git/stable/linux.git/tree/drivers/hid/usbhid/usbhid.h?h=v7.2.4).

This is an ordinary-operation kernel bound, not a formal real-time or hard-cancellation guarantee. Timeout recovery may continue waiting in `usb_kill_urb` without a second deadline. A bounded keyboard plan has `5 + set_color_frames` HID operations. Counts 1..=15 use six operations (about 30 seconds); the full 85-target catalog uses six color frames and eleven operations (about 55 seconds), plus scheduling and cancellation delay. If a later color frame fails, earlier frames may already have left a partial applied state. No worker-thread or subprocess pseudo-timeout is used. No operation is retried or replayed automatically, and the nonblocking guard prevents overlapping keyboard executions only within one process.

## Optional device access rules

[`contrib/udev/70-alienrgb.rules`](contrib/udev/70-alienrgb.rules) narrowly grants `TAG+="uaccess"` with mode `0660` to keyboard hidraw `0d62:d2b1` interface `00` and AW-ELC USB `187c:0551` with its expected HID interface signature. It does not match unrelated hidraw or USB devices. Descriptor hashes, physical topology, endpoints, and driver state remain runtime-only checks. Installation and rule reload are manual; see [`contrib/udev/README.md`](contrib/udev/README.md). The project does not install or reload rules.

On the current confirmed host, the repository rule is installed byte-identically and active. `/dev/hidraw1` is root-owned mode `0660`, while its active POSIX ACL grants the current user effective read/write access. The guarded non-root transport has physically validated the exact static-red one-key, one-frame, two-frame, and full-catalog operations recorded above. Only Escape, F1, W, and Space establish individual keyboard address identity; the uniform `all` result establishes full-set coverage, not every individual mapping. Do not run `alienrgb` with `sudo`.
