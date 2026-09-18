use alienrgb::cli::{parse, run_with_inventory, usage, CliCommand, RequestedDevice};
use alienrgb::model::{
    DescriptorStatus, DeviceKind, DeviceStatus, DeviceSummary, DmiIdentity, EffectiveAccess,
    HidrawInfo, HidrawNodeState, PermissionEstimate,
};
use alienrgb::profile::{DescriptorEvidence, KEYBOARD_DESCRIPTOR_SHA256};

#[test]
fn power_profile_parser_separates_dry_run_and_confirmed_apply() {
    let cli = parse(args([
        "power-profile",
        "--color",
        "#Aa00Ff",
        "--dry-run",
        "--json",
    ]))
    .unwrap();
    assert!(matches!(
        cli.command,
        CliCommand::PowerProfile {
            color,
            dry_run: true,
            apply: false,
            ..
        } if color.hex() == "#aa00ff"
    ));
    assert!(cli.json);
    assert!(usage().contains("power-profile --color <RRGGBB|#RRGGBB> --dry-run [--json]"));
    let apply = parse(args([
        "power-profile",
        "--color",
        "ff0000",
        "--apply",
        "--experimental",
        "--confirm-power-profile-write",
        "--json",
    ]))
    .unwrap();
    assert!(matches!(
        apply.command,
        CliCommand::PowerProfile {
            dry_run: false,
            apply: true,
            experimental: true,
            confirm_power_profile_write: true,
            ..
        }
    ));

    for invalid in [
        vec!["power-profile", "--dry-run"],
        vec!["power-profile", "--color", "ff0000"],
        vec![
            "power-profile",
            "--color",
            "ff0000",
            "--color",
            "00ff00",
            "--dry-run",
        ],
        vec![
            "power-profile",
            "--color",
            "ff0000",
            "--dry-run",
            "--dry-run",
        ],
        vec!["power-profile", "--color", "ff0000", "--dry-run", "--apply"],
        vec!["power-profile", "--color", "ff0000", "--apply"],
        vec![
            "power-profile",
            "--color",
            "ff0000",
            "--apply",
            "--experimental",
        ],
        vec![
            "power-profile",
            "--color",
            "ff0000",
            "--apply",
            "--confirm-power-profile-write",
        ],
        vec![
            "power-profile",
            "--color",
            "ff0000",
            "--apply",
            "--experimental",
            "--confirm-live-write",
        ],
        vec![
            "power-profile",
            "--color",
            "#ff0000",
            "--apply",
            "--experimental",
            "--confirm-power-profile-write",
        ],
        vec![
            "power-profile",
            "--color",
            "ff0000",
            "--apply",
            "--experimental",
            "--confirm-power-profile-write",
            "--apply",
        ],
        vec![
            "power-profile",
            "--color",
            "ff0000",
            "--apply",
            "--experimental",
            "--confirm-power-profile-write",
            "--experimental",
        ],
        vec![
            "power-profile",
            "--color",
            "ff0000",
            "--apply",
            "--experimental",
            "--confirm-power-profile-write",
            "--confirm-power-profile-write",
        ],
        vec![
            "power-profile",
            "--color",
            "ff0000",
            "--dry-run",
            "--experimental",
        ],
        vec![
            "power-profile",
            "--color",
            "ff0000",
            "--dry-run",
            "--confirm-live-write",
        ],
        vec!["power-profile", "--color", "ff0000", "--dry-run", "extra"],
        vec![
            "power-profile",
            "--color",
            "ff0000",
            "--dry-run",
            "--unknown",
        ],
    ] {
        assert!(
            parse(invalid.iter().copied().map(str::to_string)).is_err(),
            "{invalid:?}"
        );
    }
}

#[test]
fn power_profile_gate_and_reports_are_exact_and_transport_free() {
    let cli = parse(args([
        "power-profile",
        "--color",
        "123456",
        "--dry-run",
        "--json",
    ]))
    .unwrap();
    let (dmi, devices) = supported_inventory();
    let output = run_with_inventory(cli.clone(), dmi.clone(), devices.clone()).unwrap();
    let json: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["command"], "power_profile");
    assert_eq!(json["mode"], "dry_run");
    assert_eq!(json["profile_kind"], "power_state_profile");
    assert_eq!(json["transport_performed"], false);
    assert_eq!(json["controller"], "187c:0551");
    assert_eq!(json["power_logical_id"], 4);
    assert_eq!(json["validation"], "mapping_derived_unvalidated");
    assert_eq!(json["color"], "#123456");
    assert_eq!(json["color_applies_to"], serde_json::json!(["ac", "dc"]));
    assert_eq!(json["packet_count"], 34);
    assert_eq!(json["states"].as_array().unwrap().len(), 6);
    assert_eq!(json["states"][0]["id"], 0x5b);
    assert_eq!(json["states"][0]["name"], "ac_sleep");
    assert_eq!(json["states"][0]["packet_count"], 6);
    assert_eq!(json["states"][5]["id"], 0x60);
    assert_eq!(json["plan"]["steps"].as_array().unwrap().len(), 34);
    assert_eq!(json["persistence"], "unknown_power_profile_state");
    assert_eq!(json["state_restore_available"], false);
    assert_eq!(json["readback_available"], false);

    let red_cli = parse(args([
        "power-profile",
        "--color",
        "ff0000",
        "--dry-run",
        "--json",
    ]))
    .unwrap();
    let (red_dmi, red_devices) = supported_inventory();
    let red_output = run_with_inventory(red_cli, red_dmi, red_devices).unwrap();
    let red_json: serde_json::Value = serde_json::from_str(&red_output).unwrap();
    assert_eq!(
        red_json["validation"],
        "exact_power_profile_static_red_battery_on_observed"
    );
    assert_eq!(
        red_json["plan"]["validation"],
        "mapping_derived_unvalidated"
    );

    let mut bad_bios = dmi.clone();
    bad_bios.bios_version = Some("1.22.0".into());
    assert!(run_with_inventory(cli.clone(), bad_bios, devices.clone())
        .unwrap_err()
        .to_string()
        .contains("BIOS 1.21.0"));
    let mut missing = devices;
    missing.retain(|device| device.kind != DeviceKind::Chassis);
    assert!(run_with_inventory(cli, dmi, missing)
        .unwrap_err()
        .to_string()
        .contains("187c:0551"));

    let human_cli = parse(args(["power-profile", "--color", "123456", "--dry-run"])).unwrap();
    let (dmi, devices) = supported_inventory();
    let human = run_with_inventory(human_cli, dmi, devices).unwrap();
    for text in [
        "POWER PROFILE DRY RUN — NO HARDWARE TRANSPORT",
        "power_state_profile",
        "MappingDerivedUnvalidated",
        "Power logical ID: 4",
        "both AC and DC",
        "Packets: 34",
        "transport_performed=false",
        "persistence=unknown_power_profile_state",
        "restore unavailable",
        "readback unavailable",
    ] {
        assert!(human.contains(text), "missing {text:?} in {human}");
    }
    assert!(!human.contains("transport_performed=true"));

    let red_human_cli = parse(args(["power-profile", "--color", "ff0000", "--dry-run"])).unwrap();
    let (red_dmi, red_devices) = supported_inventory();
    let red_human = run_with_inventory(red_human_cli, red_dmi, red_devices).unwrap();
    assert!(red_human.contains("ExactPowerProfileStaticRedBatteryOnObserved"));
    assert!(!red_human.contains("validation=mapping-derived-unvalidated"));
    assert!(!red_human.contains("all six states were visually observed"));
}

#[test]
fn ordinary_set_rejects_power_but_leaves_touchpad_and_back_unchanged() {
    for target in ["power", "power-button", "all"] {
        for mode in [
            vec!["--dry-run"],
            vec!["--apply", "--experimental", "--confirm-live-write"],
        ] {
            let mut command = vec![
                "set", "--device", "chassis", "--target", target, "--color", "ff0000",
            ];
            command.extend(mode);
            let error = parse(command.into_iter().map(str::to_string)).unwrap_err();
            assert!(error.contains("power-profile"));
            assert!(error.contains("dry-run"));
        }
    }
    for target in ["touchpad", "back"] {
        assert!(parse(args([
            "set",
            "--device",
            "chassis",
            "--target",
            target,
            "--color",
            "ff0000",
            "--dry-run",
        ]))
        .is_ok());
    }
}

#[test]
fn parses_zones_filters_and_documents_usage() {
    let cli = parse(args(["zones", "--device", "keyboard", "--json"])).unwrap();
    assert!(matches!(
        cli.command,
        CliCommand::Zones {
            device: Some(RequestedDevice::Keyboard)
        }
    ));
    assert!(cli.json);
    assert!(usage().contains("zones [--device keyboard|chassis] [--json]"));
    assert!(usage().contains("set --device <keyboard|chassis>"));
    assert!(usage().contains("--dry-run"));
}

#[test]
fn parses_repeated_and_comma_separated_targets_and_colors() {
    let cli = parse(args([
        "set",
        "--device",
        "keyboard",
        "--target",
        "f2,esc",
        "--target",
        "f1",
        "--color",
        "#Aa00Ff",
        "--dry-run",
        "--json",
    ]))
    .unwrap();
    let CliCommand::Set {
        device,
        targets,
        color,
        dry_run,
        ..
    } = cli.command
    else {
        panic!("expected set command");
    };
    assert_eq!(device, RequestedDevice::Keyboard);
    assert_eq!(targets, vec!["f2", "esc", "f1"]);
    assert_eq!(color.hex(), "#aa00ff");
    assert!(dry_run);
}

#[test]
fn rejects_missing_modes_and_unknown_write_flags() {
    let error = parse(args([
        "set", "--device", "keyboard", "--target", "esc", "--color", "ff0000",
    ]))
    .unwrap_err();
    assert!(error.contains("exactly one of --dry-run or --apply"));

    let error = parse(args([
        "set",
        "--device",
        "keyboard",
        "--target",
        "esc",
        "--color",
        "ff0000",
        "--dry-run",
        "--apply",
    ]))
    .unwrap_err();
    assert!(error.contains("mutually exclusive"));

    for forbidden in ["--write", "--live"] {
        let error = parse(args([
            "set",
            "--device",
            "keyboard",
            "--target",
            "esc",
            "--color",
            "ff0000",
            "--dry-run",
            forbidden,
        ]))
        .unwrap_err();
        assert!(error.contains("unexpected argument"));
    }
}

#[test]
fn rejects_missing_empty_and_malformed_set_values() {
    for command in [
        vec![
            "set",
            "--device",
            "keyboard",
            "--target",
            "esc",
            "--dry-run",
        ],
        vec![
            "set",
            "--device",
            "keyboard",
            "--color",
            "ff0000",
            "--dry-run",
        ],
        vec!["set", "--target", "esc", "--color", "ff0000", "--dry-run"],
        vec![
            "set",
            "--device",
            "keyboard",
            "--target",
            "",
            "--color",
            "ff0000",
            "--dry-run",
        ],
        vec![
            "set",
            "--device",
            "keyboard",
            "--target",
            "esc,,f1",
            "--color",
            "ff0000",
            "--dry-run",
        ],
    ] {
        assert!(parse(command.into_iter().map(str::to_string)).is_err());
    }
    for color in ["", "fff", "gg0000", "##ff0000", "ff000000"] {
        let error = parse(args([
            "set",
            "--device",
            "keyboard",
            "--target",
            "esc",
            "--color",
            color,
            "--dry-run",
        ]))
        .unwrap_err();
        assert!(error.contains("color"));
    }
}

#[test]
fn rejects_duplicate_unknown_mixed_device_and_conflicting_all_targets() {
    let inventory = supported_inventory();
    for (targets, expected) in [
        ("escape,esc", "duplicate target 'escape'"),
        ("unknown-zone", "unknown target 'unknown-zone'"),
        ("touchpad", "belongs to device 'chassis'"),
        ("all,esc", "'all' cannot be combined"),
    ] {
        let cli = parse(args([
            "set",
            "--device",
            "keyboard",
            "--target",
            targets,
            "--color",
            "102030",
            "--dry-run",
        ]))
        .unwrap();
        let error = run_with_inventory(cli, inventory.0.clone(), inventory.1.clone()).unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
    }
}

#[test]
fn gates_dry_run_plans_on_dmi_usb_and_confirmed_keyboard_hash() {
    let cli = keyboard_set_cli();
    let (mut dmi, devices) = supported_inventory();
    dmi.supported = false;
    assert!(run_with_inventory(cli.clone(), dmi, devices.clone())
        .unwrap_err()
        .to_string()
        .contains("DMI"));

    let (dmi, mut devices) = supported_inventory();
    devices.retain(|device| device.kind != DeviceKind::Keyboard);
    assert!(run_with_inventory(cli.clone(), dmi, devices)
        .unwrap_err()
        .to_string()
        .contains("0d62:d2b1"));

    let (dmi, mut devices) = supported_inventory();
    devices[0].vid = "ffff".into();
    assert!(run_with_inventory(cli.clone(), dmi, devices)
        .unwrap_err()
        .to_string()
        .contains("0d62:d2b1"));

    let (dmi, mut devices) = supported_inventory();
    devices[0].descriptor.as_mut().unwrap().evidence = DescriptorEvidence::CompatibleSignature;
    assert!(run_with_inventory(cli, dmi, devices)
        .unwrap_err()
        .to_string()
        .contains("confirmed descriptor hash"));

    let chassis_cli = parse(args([
        "set",
        "--device",
        "chassis",
        "--target",
        "back",
        "--color",
        "ff0000",
        "--dry-run",
    ]))
    .unwrap();
    let (dmi, mut devices) = supported_inventory();
    devices.retain(|device| device.kind != DeviceKind::Chassis);
    assert!(run_with_inventory(chassis_cli, dmi, devices)
        .unwrap_err()
        .to_string()
        .contains("187c:0551"));
}

#[test]
fn keyboard_json_plan_is_stable_resolved_and_never_transported() {
    let cli = parse(args([
        "set",
        "--device",
        "keyboard",
        "--target",
        "f2,esc,f1",
        "--color",
        "0A0B0C",
        "--dry-run",
        "--json",
    ]))
    .unwrap();
    let (dmi, devices) = supported_inventory();
    let output = run_with_inventory(cli, dmi, devices).unwrap();
    let json: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["mode"], "dry_run");
    assert_eq!(json["transport_performed"], false);
    assert_eq!(json["requested_device"], "keyboard");
    assert_eq!(json["plan"]["family"], "alien_fx_api_v5");
    assert_eq!(json["plan"]["assignments"][0]["target"], "escape");
    assert_eq!(json["plan"]["assignments"][1]["target"], "f1");
    assert_eq!(json["plan"]["assignments"][2]["target"], "f2");
    assert_eq!(json["plan"]["steps"][0]["transfer"], "hid_feature_write");
    assert_eq!(json["plan"]["steps"][0]["caller_buffer_length"], 64);
}

#[test]
fn chassis_plan_and_human_output_distinguish_target_validation_evidence() {
    let cli = parse(args([
        "set",
        "--device",
        "chassis",
        "--target",
        "haptic,back",
        "--color",
        "ff0000",
        "--dry-run",
    ]))
    .unwrap();
    let (dmi, devices) = supported_inventory();
    let output = run_with_inventory(cli, dmi, devices).unwrap();
    assert!(output.starts_with("DRY RUN — NO HARDWARE TRANSPORT\n"));
    assert!(output.contains("MixedChassisTargetEvidence"));
    assert!(output.contains("ExactTouchpadStaticRedLiveValidated"));
    assert!(output.contains("ExactBackStaticRedLiveValidated"));
    assert!(output.contains("Back/chassis logical ID 2"));
    assert!(output.contains("BIOS 1.21.0"));
    assert!(output.contains("33-byte"));

    let json_cli = parse(args([
        "set",
        "--device",
        "chassis",
        "--target",
        "haptic,back",
        "--color",
        "ff0000",
        "--dry-run",
        "--json",
    ]))
    .unwrap();
    let (dmi, devices) = supported_inventory();
    let json: serde_json::Value =
        serde_json::from_str(&run_with_inventory(json_cli, dmi, devices).unwrap()).unwrap();
    assert_eq!(json["requested_device"], "chassis");
    assert_eq!(json["plan"]["family"], "alien_fx_api_v4");
    assert_eq!(json["plan"]["steps"][0]["transfer"], "usb_output");
    assert_eq!(json["plan"]["steps"][0]["on_wire_length"], 33);
    assert_eq!(json["plan"]["validation"], "mixed_chassis_target_evidence");
    assert_eq!(
        json["plan"]["assignments"][0]["validation"],
        "exact_touchpad_static_red_live_validated"
    );
    assert_eq!(json["plan"]["assignments"][0]["target"], "touchpad");
    assert_eq!(
        json["plan"]["assignments"][1]["validation"],
        "exact_back_static_red_live_validated"
    );
    assert!(json["plan"]["assumptions"][0]["detail"]
        .as_str()
        .unwrap()
        .contains("visually confirmed red"));
}

#[test]
fn touchpad_and_back_static_red_are_exactly_validated_but_green_is_not() {
    let red_json: serde_json::Value =
        serde_json::from_str(&chassis_dry_run("touchpad", "ff0000", true)).unwrap();
    let green_json: serde_json::Value =
        serde_json::from_str(&chassis_dry_run("haptic", "00ff00", true)).unwrap();
    assert_eq!(
        red_json["plan"]["assignments"][0]["validation"],
        "exact_touchpad_static_red_live_validated"
    );
    assert_eq!(
        green_json["plan"]["assignments"][0]["validation"],
        "mapping_derived_unvalidated"
    );
    assert_eq!(
        green_json["plan"]["validation"],
        "mapping_derived_unvalidated"
    );

    let back_json: serde_json::Value =
        serde_json::from_str(&chassis_dry_run("back", "ff0000", true)).unwrap();
    assert_eq!(
        back_json["plan"]["assignments"][0]["validation"],
        "exact_back_static_red_live_validated"
    );
    assert_eq!(
        back_json["plan"]["validation"],
        "exact_back_static_red_live_validated"
    );

    let back_green_json: serde_json::Value =
        serde_json::from_str(&chassis_dry_run("chassis", "00ff00", true)).unwrap();
    assert_eq!(
        back_green_json["plan"]["validation"],
        "mapping_derived_unvalidated"
    );

    let red_human = chassis_dry_run("touchpad", "ff0000", false);
    let green_human = chassis_dry_run("touchpad", "00ff00", false);
    let red_assignment = red_human
        .lines()
        .find(|line| line.starts_with("- target=touchpad"))
        .unwrap();
    let green_assignment = green_human
        .lines()
        .find(|line| line.starts_with("- target=touchpad"))
        .unwrap();
    assert!(red_assignment.contains("ExactTouchpadStaticRedLiveValidated"));
    assert!(green_assignment.contains("MappingDerivedUnvalidated"));
    assert!(green_human.contains("Any touchpad or back color other than #ff0000"));
}

#[test]
fn info_capabilities_enumerate_logical_targets_and_correct_v4_framing() {
    let cli = parse(args(["info", "--json"])).unwrap();
    let (dmi, devices) = supported_inventory();
    let json: serde_json::Value =
        serde_json::from_str(&run_with_inventory(cli, dmi, devices).unwrap()).unwrap();
    let capabilities = json["capabilities"].as_array().unwrap();
    assert!(capabilities[0]["logical_targets"]
        .as_array()
        .unwrap()
        .iter()
        .any(|target| target == "escape"));
    assert_eq!(
        capabilities[0]["validation_summary"],
        "One-, two-, and six-frame static-red transport plus whole-keyboard coverage were physically validated on the exact Alienware m16 R2 / BIOS 1.21.0 / 0d62:d2b1 / pinned-descriptor profile; only Escape/F1/W/Space were individually discriminated. Other individual logical IDs, colors, effects, models, and BIOS versions remain unvalidated."
    );
    assert_eq!(
        capabilities[1]["logical_targets"],
        serde_json::json!(["touchpad", "back", "power"])
    );
    assert!(capabilities[1]["transport"]
        .as_str()
        .unwrap()
        .contains("33-byte direct-libusb"));
    assert!(capabilities[1]["transport"]
        .as_str()
        .unwrap()
        .contains("34-byte caller buffer"));
    assert_eq!(
        capabilities[1]["write_support"],
        "experimental_apply_mixed_target_evidence"
    );
    assert!(capabilities[1]["validation_summary"]
        .as_str()
        .unwrap()
        .contains("Touchpad/haptic logical ID 0"));
    assert!(capabilities[1]["validation_summary"]
        .as_str()
        .unwrap()
        .contains("back/chassis logical ID 2"));
    assert!(capabilities[1]["validation_summary"]
        .as_str()
        .unwrap()
        .contains("power-profile #ff0000"));
    assert!(capabilities[1]["validation_summary"]
        .as_str()
        .unwrap()
        .contains("battery-on/discharging"));
    assert!(capabilities[1]["validation_summary"]
        .as_str()
        .unwrap()
        .contains("other five states remain visually unobserved"));

    let human_cli = parse(args(["info"])).unwrap();
    let (dmi, devices) = supported_inventory();
    let human = run_with_inventory(human_cli, dmi, devices).unwrap();
    assert!(human.contains("Touchpad/haptic logical ID 0"));
    assert!(human.contains("back/chassis logical ID 2"));
    assert!(human.contains("power-profile #ff0000"));
    assert!(human.contains("other five states remain visually unobserved"));
}

#[test]
fn zones_describe_logical_targets_behind_two_usb_controllers() {
    let cli = parse(args(["zones", "--json"])).unwrap();
    let output = run_with_inventory(cli, unsupported_dmi(), Vec::new()).unwrap();
    let json: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["command"], "zones");
    assert_eq!(json["usb_controller_count"], 2);
    assert!(json["note"]
        .as_str()
        .unwrap()
        .contains("logical RGB targets"));
    let zones = json["zones"].as_array().unwrap();
    assert!(zones.len() > 3);
    let touchpad = zones
        .iter()
        .find(|zone| zone["target"] == "touchpad")
        .unwrap();
    assert_eq!(touchpad["validation"], "mapping_derived_unvalidated");
    assert_eq!(touchpad["known_validation"]["mode"], "static_color");
    assert_eq!(touchpad["known_validation"]["color"], "#ff0000");
    assert_eq!(touchpad["known_validation"]["logical_id"], 0);
    assert_eq!(touchpad["known_validation"]["bios_version"], "1.21.0");
    let back = zones.iter().find(|zone| zone["target"] == "back").unwrap();
    assert_eq!(back["validation"], "mapping_derived_unvalidated");
    assert_eq!(back["known_validation"]["mode"], "static_color");
    assert_eq!(back["known_validation"]["color"], "#ff0000");
    assert_eq!(back["known_validation"]["logical_id"], 2);
    assert!(back["known_validation"]["evidence"]
        .as_str()
        .unwrap()
        .contains("only rear lighting became red"));
    let power = zones.iter().find(|zone| zone["target"] == "power").unwrap();
    assert_eq!(power["validation"], "mapping_derived_unvalidated");
    assert_eq!(
        power["known_validation"]["mode"],
        "power_profile_equal_color"
    );
    assert_eq!(power["known_validation"]["color"], "#ff0000");
    assert_eq!(power["known_validation"]["logical_id"], 4);
    assert!(power["known_validation"]["evidence"]
        .as_str()
        .unwrap()
        .contains("BAT0 Discharging"));

    let human_cli = parse(args(["zones", "--device", "chassis"])).unwrap();
    let human = run_with_inventory(human_cli, unsupported_dmi(), Vec::new()).unwrap();
    assert!(human.contains("known_validation: mode=StaticColor color=#ff0000"));
    assert!(human.contains("bios=1.21.0"));
    assert!(human.contains("target=back"));
    assert!(human.contains("only rear lighting became red"));
    assert!(human.contains("known_validation: mode=PowerProfileEqualColor color=#ff0000"));
    assert!(human.contains("battery-on/discharging visible behavior"));
}

#[test]
fn keyboard_zones_serialize_only_four_individual_validation_records() {
    let cli = parse(args(["zones", "--device", "keyboard", "--json"])).unwrap();
    let output = run_with_inventory(cli, unsupported_dmi(), Vec::new()).unwrap();
    let json: serde_json::Value = serde_json::from_str(&output).unwrap();
    let zones = json["zones"].as_array().unwrap();
    let recorded = zones
        .iter()
        .filter(|zone| !zone["known_validation"].is_null())
        .collect::<Vec<_>>();
    assert_eq!(
        recorded
            .iter()
            .map(|zone| (
                zone["target"].as_str().unwrap(),
                zone["logical_id"].as_u64().unwrap()
            ))
            .collect::<Vec<_>>(),
        [("escape", 0), ("f1", 1), ("w", 43), ("space", 106)]
    );
    for target in ["f2", "q"] {
        let zone = zones.iter().find(|zone| zone["target"] == target).unwrap();
        assert!(zone["known_validation"].is_null());
        assert!(zone["validation_detail"]
            .as_str()
            .unwrap()
            .contains("whole-keyboard uniform static red coverage"));
    }
}

#[test]
fn doctor_uses_effective_acl_access_and_ignores_chassis_hidraw_absence() {
    let (dmi, mut devices) = supported_inventory();
    devices[0].hidraw.push(HidrawInfo {
        path: "/dev/hidraw-test".into(),
        interface_number: Some("00".into()),
        node_state: HidrawNodeState::Present,
        read_access: PermissionEstimate::DeniedByModeBits,
        write_access: PermissionEstimate::DeniedByModeBits,
        access_basis: "unix_mode_bits_estimate",
        effective_read_access: EffectiveAccess::Allowed,
        effective_write_access: EffectiveAccess::Allowed,
        effective_access_basis: "faccessat_at_eaccess",
    });
    let human_dmi = dmi.clone();
    let human_devices = devices.clone();
    let cli = parse(args(["doctor", "--json"])).unwrap();
    let json: serde_json::Value =
        serde_json::from_str(&run_with_inventory(cli, dmi, devices).unwrap()).unwrap();
    let permission = json["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|finding| finding["code"] == "hidraw_permissions")
        .unwrap();
    assert_eq!(permission["level"], "ok");
    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["keyboard_ready_for_live_write"], true);
    assert_eq!(json["chassis_ready_for_live_write"], true);
    assert_eq!(json["ready_for_writes"], true);
    assert!(json["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(
            |finding| finding["code"] == "confirmed_bios_live_profile" && finding["level"] == "ok"
        ));
    assert!(permission["message"]
        .as_str()
        .unwrap()
        .contains("mode_bits"));
    assert!(permission["message"]
        .as_str()
        .unwrap()
        .contains("effective"));
    assert!(permission["action"].is_null());
    assert!(!json["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|finding| finding["code"] == "hidraw_missing"));

    let human =
        run_with_inventory(parse(args(["doctor"])).unwrap(), human_dmi, human_devices).unwrap();
    assert!(human.contains("mode_bits(read=denied_by_mode_bits"));
    assert!(human.contains("effective(read=allowed, write=allowed"));
    assert!(human.contains("Keyboard live-write readiness: technically ready"));
    assert!(human.contains("Chassis live-write readiness: technically ready before acquisition"));
    assert!(human.contains("only Escape/F1/W/Space were individually discriminated"));
    assert!(human.contains("touchpad static red, back static red, and the dedicated equal-color power-profile #ff0000 only during battery-on/discharging"));
    assert!(human.contains("the other five power states"));
    assert!(human.contains("ordinary power static/address semantics"));
    assert!(!human.contains("only the exact touchpad"));
    assert!(!human.contains("all other assignments remain unvalidated"));
}

#[test]
fn doctor_requires_confirmed_bios_and_complete_chassis_preopen_identity() {
    for bios_version in [None, Some("1.22.0".into())] {
        let (mut dmi, devices) = supported_inventory();
        dmi.bios_version = bios_version;
        let json: serde_json::Value = serde_json::from_str(
            &run_with_inventory(parse(args(["doctor", "--json"])).unwrap(), dmi, devices).unwrap(),
        )
        .unwrap();
        assert_eq!(json["keyboard_ready_for_live_write"], false);
        assert_eq!(json["chassis_ready_for_live_write"], false);
        assert_eq!(json["ready_for_writes"], false);
        let bios = json["findings"]
            .as_array()
            .unwrap()
            .iter()
            .find(|finding| finding["code"] == "confirmed_bios_live_profile")
            .unwrap();
        assert_eq!(bios["level"], "error");
    }

    for defect in ["bus", "port", "interface"] {
        let (dmi, mut devices) = supported_inventory();
        let chassis = devices
            .iter_mut()
            .find(|device| device.kind == DeviceKind::Chassis)
            .unwrap();
        match defect {
            "bus" => chassis.bus_number = None,
            "port" => chassis.port_path = Some(Vec::new()),
            "interface" => chassis.interface_number = Some("01".into()),
            _ => unreachable!(),
        }
        let json: serde_json::Value = serde_json::from_str(
            &run_with_inventory(parse(args(["doctor", "--json"])).unwrap(), dmi, devices).unwrap(),
        )
        .unwrap();
        assert_eq!(json["chassis_ready_for_live_write"], false, "{defect}");
        assert_eq!(json["ready_for_writes"], false, "{defect}");
        let finding = json["findings"]
            .as_array()
            .unwrap()
            .iter()
            .find(|finding| finding["code"] == "chassis_live_profile")
            .unwrap();
        assert_eq!(finding["level"], "error", "{defect}");
    }
}

#[test]
fn doctor_distinguishes_missing_and_unknown_effective_access() {
    for (node_state, effective, expected_action) in [
        (
            HidrawNodeState::Missing,
            EffectiveAccess::Missing,
            "Confirm that udev created",
        ),
        (
            HidrawNodeState::MetadataUnavailable,
            EffectiveAccess::Unknown,
            "Effective access could not be determined",
        ),
        (
            HidrawNodeState::Present,
            EffectiveAccess::Denied,
            "verify the installed narrow udev rule and session ACL",
        ),
    ] {
        let (dmi, mut devices) = supported_inventory();
        devices[0].hidraw.push(HidrawInfo {
            path: "/dev/hidraw-test".into(),
            interface_number: Some("00".into()),
            node_state,
            read_access: PermissionEstimate::Unknown,
            write_access: PermissionEstimate::Unknown,
            access_basis: "not_inspected",
            effective_read_access: effective,
            effective_write_access: effective,
            effective_access_basis: "faccessat_at_eaccess",
        });
        let cli = parse(args(["doctor", "--json"])).unwrap();
        let json: serde_json::Value =
            serde_json::from_str(&run_with_inventory(cli, dmi, devices).unwrap()).unwrap();
        let finding = json["findings"]
            .as_array()
            .unwrap()
            .iter()
            .find(|finding| finding["code"] == "hidraw_permissions")
            .unwrap();
        assert_eq!(finding["level"], "warning");
        assert!(finding["action"]
            .as_str()
            .unwrap()
            .contains(expected_action));
    }
}

#[test]
fn keyboard_all_expansion_is_deterministic() {
    let cli = parse(args([
        "set",
        "--device",
        RequestedDevice::Keyboard.as_str(),
        "--target",
        "all",
        "--color",
        "010203",
        "--dry-run",
        "--json",
    ]))
    .unwrap();
    let (dmi, devices) = supported_inventory();
    let output = run_with_inventory(cli, dmi, devices).unwrap();
    let json: serde_json::Value = serde_json::from_str(&output).unwrap();
    let ids = json["plan"]["assignments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|assignment| assignment["logical_id"].as_u64().unwrap())
        .collect::<Vec<_>>();
    assert!(ids.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn binary_help_and_missing_mode_exit_before_discovery() {
    let binary = env!("CARGO_BIN_EXE_alienrgb");
    let help = std::process::Command::new(binary)
        .arg("--help")
        .output()
        .unwrap();
    assert_eq!(help.status.code(), Some(2));
    assert!(String::from_utf8(help.stderr)
        .unwrap()
        .contains("alienrgb zones"));

    let rejected = std::process::Command::new(binary)
        .args([
            "set", "--device", "keyboard", "--target", "esc", "--color", "ff0000",
        ])
        .output()
        .unwrap();
    assert_eq!(rejected.status.code(), Some(2));
    assert!(String::from_utf8(rejected.stderr)
        .unwrap()
        .contains("exactly one of --dry-run or --apply"));
}

fn chassis_dry_run(target: &str, color: &str, json: bool) -> String {
    let mut arguments = vec![
        "set",
        "--device",
        "chassis",
        "--target",
        target,
        "--color",
        color,
        "--dry-run",
    ];
    if json {
        arguments.push("--json");
    }
    let cli = parse(arguments.into_iter().map(str::to_string)).unwrap();
    let (dmi, devices) = supported_inventory();
    run_with_inventory(cli, dmi, devices).unwrap()
}

fn keyboard_set_cli() -> alienrgb::cli::Cli {
    parse(args([
        "set",
        "--device",
        "keyboard",
        "--target",
        "esc",
        "--color",
        "ff0000",
        "--dry-run",
    ]))
    .unwrap()
}

#[test]
fn set_all_json_parser_failures_are_standalone_versioned_preflight_json() {
    let binary = env!("CARGO_BIN_EXE_alienrgb");
    for missing in [
        "--experimental",
        "--confirm-live-write",
        "--confirm-power-profile-write",
        "--confirm-set-all-write",
    ] {
        let arguments = [
            "set-all",
            "--color",
            "ff69b4",
            "--apply",
            "--experimental",
            "--confirm-live-write",
            "--confirm-power-profile-write",
            "--confirm-set-all-write",
            "--json",
        ];
        let output = std::process::Command::new(binary)
            .args(
                arguments
                    .into_iter()
                    .filter(|argument| *argument != missing),
            )
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "missing {missing}");
        assert!(output.stdout.is_empty());
        let json: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(json["schema_version"], 1);
        assert_eq!(json["command"], "set_all");
        assert_eq!(json["overall_status"], "preflight_failure");
        assert_eq!(json["failure_code"], "invalid_arguments");
        assert_eq!(json["transport_attempted"], false);
        assert_eq!(json["transport_performed"], false);
        assert!(json["message"].as_str().unwrap().contains(missing));
    }
}

fn supported_inventory() -> (DmiIdentity, Vec<DeviceSummary>) {
    (
        DmiIdentity {
            vendor: Some("Alienware".into()),
            product: Some("Alienware m16 R2".into()),
            bios_version: Some("1.21.0".into()),
            supported: true,
        },
        vec![
            DeviceSummary {
                kind: DeviceKind::Keyboard,
                status: DeviceStatus::Found,
                vid: "0d62".into(),
                pid: "d2b1".into(),
                manufacturer: Some("DELL Technologies".into()),
                product: Some("Keyboard".into()),
                serial: None,
                sysfs_name: Some("1-1".into()),
                bus_number: Some(1),
                port_path: Some(vec![1]),
                interface_number: Some("00".into()),
                hidraw: Vec::new(),
                descriptor: Some(DescriptorStatus {
                    evidence: DescriptorEvidence::HashMatch,
                    sha256: Some(KEYBOARD_DESCRIPTOR_SHA256.into()),
                    expected_sha256: KEYBOARD_DESCRIPTOR_SHA256,
                    interface_number: Some("00".into()),
                    sysfs_path: Some("/sys/mock/report_descriptor".into()),
                }),
            },
            DeviceSummary {
                kind: DeviceKind::Chassis,
                status: DeviceStatus::Found,
                vid: "187c".into(),
                pid: "0551".into(),
                manufacturer: None,
                product: None,
                serial: None,
                sysfs_name: Some("1-2".into()),
                bus_number: Some(1),
                port_path: Some(vec![2]),
                interface_number: None,
                hidraw: Vec::new(),
                descriptor: None,
            },
        ],
    )
}

fn unsupported_dmi() -> DmiIdentity {
    DmiIdentity {
        vendor: Some("Other".into()),
        product: Some("Other".into()),
        bios_version: None,
        supported: false,
    }
}

#[test]
fn set_all_dry_run_and_argument_failure_never_create_resume_state() {
    let binary = env!("CARGO_BIN_EXE_alienrgb");
    let unique = format!(
        "alienrgb-no-state-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let state_home = std::env::temp_dir().join(unique);
    let state_file = state_home.join("alienrgb/last-set-all-color");

    let dry_run = std::process::Command::new(binary)
        .args(["set-all", "--color", "abcdef", "--dry-run"])
        .env("XDG_STATE_HOME", &state_home)
        .env_remove("HOME")
        .output()
        .unwrap();
    assert!(dry_run.status.success());
    assert!(!state_file.exists());

    let rejected = std::process::Command::new(binary)
        .args([
            "set-all",
            "--color",
            "abcdef",
            "--apply",
            "--experimental",
            "--confirm-live-write",
            "--confirm-power-profile-write",
        ])
        .env("XDG_STATE_HOME", &state_home)
        .env_remove("HOME")
        .output()
        .unwrap();
    assert_eq!(rejected.status.code(), Some(2));
    assert!(!state_file.exists());
}

fn args<const N: usize>(values: [&str; N]) -> std::vec::IntoIter<String> {
    values
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>()
        .into_iter()
}
