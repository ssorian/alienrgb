use super::*;
use crate::model::{DescriptorStatus, DeviceStatus};
use crate::profile::{DescriptorEvidence, KEYBOARD_DESCRIPTOR_SHA256};
use std::cell::RefCell;
use std::rc::Rc;

#[test]
fn power_profile_live_fake_warns_before_executor_and_reports_complete_steps() {
    let state = SharedState::default();
    let mut executor = FakePowerProfileExecutor::success(state.clone());
    let mut warnings = FakeWarnings::new(state.clone());
    let cli = parse(strings(&[
        "power-profile",
        "--color",
        "123456",
        "--apply",
        "--experimental",
        "--confirm-power-profile-write",
    ]))
    .unwrap();

    let output =
        run_power_profile_with_services(cli, supported_inventory(), &mut executor, &mut warnings)
            .unwrap();

    assert_eq!(executor.calls, 1);
    assert_eq!(executor.colors, [Rgb::new(0x12, 0x34, 0x56)]);
    assert_eq!(state.borrow().events, ["warning", "power_execute"]);
    let warning = &warnings.messages[0];
    for text in [
        "special six-state profile",
        "power ID4",
        "34 ordered 33-byte writes",
        "all AC/battery states",
        "up to about 17 seconds",
        "partial profile",
        "unknown persistence",
        "no readback/restore",
        "no retry",
        "no status polling",
    ] {
        assert!(warning.contains(text), "missing {text:?} in {warning}");
    }
    assert!(output.contains("POWER PROFILE WRITE COMPLETED"));
    assert!(output.contains("completed_steps=34"));
    assert!(output.contains("transport_performed=true"));
    assert!(!output.contains("transport_performed=false"));

    let state = SharedState::default();
    let mut executor = FakePowerProfileExecutor::success(state.clone());
    let mut warnings = FakeWarnings::new(state);
    let cli = parse(strings(&[
        "power-profile",
        "--color",
        "123456",
        "--apply",
        "--experimental",
        "--confirm-power-profile-write",
        "--json",
    ]))
    .unwrap();
    let output =
        run_power_profile_with_services(cli, supported_inventory(), &mut executor, &mut warnings)
            .unwrap();
    let json: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert!(warnings.messages.is_empty());
    assert_eq!(json["mode"], "apply");
    assert_eq!(json["power_profile"], true);
    assert_eq!(json["transport_performed"], true);
    assert_eq!(json["executed_steps"].as_array().unwrap().len(), 34);
    assert_eq!(json["states"].as_array().unwrap().len(), 6);
    assert_eq!(json["persistence"], "unknown_power_profile_state");
    assert_eq!(json["state_restore_available"], false);
    assert_eq!(json["readback_available"], false);
}

#[test]
fn power_profile_dry_run_never_calls_executor_or_warns() {
    let state = SharedState::default();
    let mut executor = FakePowerProfileExecutor::success(state.clone());
    let mut warnings = FakeWarnings::new(state);
    let cli = parse(strings(&[
        "power-profile",
        "--color",
        "123456",
        "--dry-run",
    ]))
    .unwrap();
    let output =
        run_power_profile_with_services(cli, supported_inventory(), &mut executor, &mut warnings)
            .unwrap();
    assert_eq!(executor.calls, 0);
    assert!(warnings.messages.is_empty());
    assert!(output.starts_with("POWER PROFILE DRY RUN"));
    assert!(output.contains("transport_performed=false"));
    assert!(!output.contains("transport_performed=true"));
}

#[test]
fn keyboard_status_requires_exact_confirmation_flags_and_rejects_set_arguments() {
    assert!(parse(strings(&[
        "keyboard-status",
        "--experimental",
        "--confirm-live-query",
    ]))
    .is_ok());
    for invalid in [
        vec!["keyboard-status"],
        vec!["keyboard-status", "--experimental"],
        vec!["keyboard-status", "--confirm-live-query"],
        vec![
            "keyboard-status",
            "--experimental",
            "--confirm-live-query",
            "--experimental",
        ],
        vec![
            "keyboard-status",
            "--experimental",
            "--confirm-live-query",
            "--target",
            "esc",
        ],
        vec![
            "keyboard-status",
            "--experimental",
            "--confirm-live-query",
            "--apply",
        ],
    ] {
        assert!(parse(invalid.into_iter().map(str::to_string)).is_err());
    }
}

#[test]
fn keyboard_live_accepts_three_explicit_keys_in_one_typed_call() {
    let state = SharedState::default();
    let mut chassis_executor = FakeExecutor::success(state.clone());
    let mut keyboard_executor = FakeKeyboardExecutor::success(state.clone());
    let mut warnings = FakeWarnings::new(state);
    let cli = parse(strings(&[
        "set",
        "--device",
        "keyboard",
        "--target",
        "f2,a,arrow-right",
        "--color",
        "ff0000",
        "--apply",
        "--experimental",
        "--confirm-live-write",
        "--json",
    ]))
    .unwrap();
    let output = run_with_all_services(
        cli,
        supported_keyboard_inventory(),
        &mut chassis_executor,
        &mut keyboard_executor,
        &mut warnings,
    )
    .unwrap();
    let json: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(keyboard_executor.calls, 1);
    assert_eq!(
        keyboard_executor.intents[0]
            .targets
            .iter()
            .map(|target| (target.target, target.logical_id))
            .collect::<Vec<_>>(),
        [("f2", 2), ("a", 62), ("arrow-right", 135)]
    );
    assert_eq!(keyboard_executor.intents[0].color.hex(), "#ff0000");
    assert_eq!(json["requested_target"], "f2,a,arrow-right");
    assert_eq!(json["resolved_target"], "f2,a,arrow-right");
    assert_eq!(json["target_count"], 3);
    assert_eq!(json["set_color_frames"], 1);
    assert_eq!(json["executed_steps"].as_array().unwrap().len(), 6);
    assert_eq!(
        json["executed_steps"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|step| step["name"] == "set_color")
            .count(),
        1
    );

    let state = SharedState::default();
    let mut chassis_executor = FakeExecutor::success(state.clone());
    let mut keyboard_executor = FakeKeyboardExecutor::success(state.clone());
    let mut warnings = FakeWarnings::new(state);
    let cli = parse(strings(&[
        "set",
        "--device",
        "keyboard",
        "--target",
        "arrow-right,f2,a",
        "--color",
        "ff0000",
        "--apply",
        "--experimental",
        "--confirm-live-write",
    ]))
    .unwrap();
    let human = run_with_all_services(
        cli,
        supported_keyboard_inventory(),
        &mut chassis_executor,
        &mut keyboard_executor,
        &mut warnings,
    )
    .unwrap();
    assert!(warnings.messages[0].contains("requested_keys=arrow-right,f2,a"));
    assert!(warnings.messages[0].contains("canonical_keys=f2,a,arrow-right"));
    assert!(warnings.messages[0].contains("count=3"));
    assert!(human.contains("Targets: f2,a,arrow-right (requested arrow-right,f2,a; count 3)"));
}

#[test]
fn apply_requires_both_confirmation_flags_in_any_argument_order() {
    for args in [
        vec![
            "set", "--device", "chassis", "--target", "back", "--color", "ff0000", "--apply",
        ],
        vec![
            "set",
            "--device",
            "chassis",
            "--target",
            "back",
            "--color",
            "ff0000",
            "--apply",
            "--experimental",
        ],
        vec![
            "set",
            "--device",
            "chassis",
            "--target",
            "back",
            "--color",
            "ff0000",
            "--apply",
            "--confirm-live-write",
        ],
    ] {
        let error = parse(args.into_iter().map(str::to_string)).unwrap_err();
        assert!(error.contains("--experimental") || error.contains("--confirm-live-write"));
    }

    let cli = parse(strings(&[
        "set",
        "--confirm-live-write",
        "--color",
        "010203",
        "--target",
        "chassis",
        "--experimental",
        "--device",
        "chassis",
        "--apply",
    ]))
    .unwrap();
    assert!(matches!(cli.command, CliCommand::Set { apply: true, .. }));
}

#[test]
fn modes_and_confirmation_flags_are_strictly_separated() {
    for args in [
        vec![
            "set",
            "--device",
            "chassis",
            "--target",
            "back",
            "--color",
            "ff0000",
            "--dry-run",
            "--apply",
            "--experimental",
            "--confirm-live-write",
        ],
        vec![
            "set",
            "--device",
            "chassis",
            "--target",
            "back",
            "--color",
            "ff0000",
            "--dry-run",
            "--experimental",
        ],
        vec![
            "set",
            "--device",
            "chassis",
            "--target",
            "back",
            "--color",
            "ff0000",
            "--dry-run",
            "--confirm-live-write",
        ],
    ] {
        assert!(parse(args.into_iter().map(str::to_string)).is_err());
    }
    for duplicate in [
        "--apply",
        "--experimental",
        "--confirm-live-write",
        "--dry-run",
    ] {
        let mut args = vec![
            "set",
            "--device",
            "chassis",
            "--target",
            "back",
            "--color",
            "ff0000",
            "--apply",
            "--experimental",
            "--confirm-live-write",
        ];
        args.push(duplicate);
        assert!(parse(args.into_iter().map(str::to_string)).is_err());
    }
}

#[test]
fn dry_run_never_calls_live_executor_and_disabled_keyboard_service_fails_closed() {
    let state = SharedState::default();
    let mut executor = FakeExecutor::success(state.clone());
    let mut warnings = FakeWarnings::new(state.clone());
    let dry = parse(strings(&[
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
    run_with_services(dry, supported_inventory(), &mut executor, &mut warnings).unwrap();
    assert_eq!(executor.calls, 0);

    let keyboard = parse(strings(&[
        "set",
        "--device",
        "keyboard",
        "--target",
        "esc",
        "--color",
        "ff0000",
        "--apply",
        "--experimental",
        "--confirm-live-write",
    ]))
    .unwrap();
    let error = run_with_services(
        keyboard,
        supported_keyboard_inventory(),
        &mut executor,
        &mut warnings,
    )
    .unwrap_err();
    assert_eq!(error.code, "live_executor_unavailable");
    assert_eq!(executor.calls, 0);
}

#[test]
fn keyboard_live_gate_uses_one_typed_call_and_emits_bounded_warning() {
    let state = SharedState::default();
    let mut chassis = FakeExecutor::success(state.clone());
    let mut keyboard = FakeKeyboardExecutor::success(state.clone());
    let mut warnings = FakeWarnings::new(state.clone());
    let cli = parse(strings(&[
        "set",
        "--device",
        "keyboard",
        "--target",
        "esc",
        "--color",
        "123456",
        "--apply",
        "--experimental",
        "--confirm-live-write",
    ]))
    .unwrap();
    let output = run_with_all_services(
        cli,
        supported_keyboard_inventory(),
        &mut chassis,
        &mut keyboard,
        &mut warnings,
    )
    .unwrap();
    assert_eq!(chassis.calls, 0);
    assert_eq!(keyboard.calls, 1);
    assert_eq!(keyboard.intents[0].targets.len(), 1);
    assert_eq!(keyboard.intents[0].targets[0].target, "escape");
    assert_eq!(keyboard.intents[0].targets[0].logical_id, 0);
    assert_eq!(keyboard.intents[0].color.hex(), "#123456");
    assert_eq!(state.borrow().events, vec!["warning", "keyboard_execute"]);
    assert!(warnings.messages[0].contains("kernel-bounded feature ioctl"));
    assert!(warnings.messages[0].contains("up to about 30 seconds"));
    assert!(warnings.messages[0].contains("canonical_keys=escape"));
    assert!(warnings.messages[0].contains("color=#123456"));
    assert!(output.contains("KEYBOARD WRITE COMPLETED"));
}

#[test]
fn keyboard_live_json_reports_complete_steps_without_warning_stdout() {
    let state = SharedState::default();
    let mut chassis = FakeExecutor::success(state.clone());
    let mut keyboard = FakeKeyboardExecutor::success(state.clone());
    let mut warnings = FakeWarnings::new(state);
    let cli = parse(strings(&[
        "set",
        "--device",
        "keyboard",
        "--target",
        "f1",
        "--color",
        "010203",
        "--apply",
        "--experimental",
        "--confirm-live-write",
        "--json",
    ]))
    .unwrap();
    let output = run_with_all_services(
        cli,
        supported_keyboard_inventory(),
        &mut chassis,
        &mut keyboard,
        &mut warnings,
    )
    .unwrap();
    let json: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert!(warnings.messages.is_empty());
    assert_eq!(json["requested_device"], "keyboard");
    assert_eq!(json["controller_vid"], "0d62");
    assert_eq!(json["controller_pid"], "d2b1");
    assert_eq!(json["transport_performed"], true);
    assert_eq!(json["executed_steps"].as_array().unwrap().len(), 6);
    assert_eq!(json["executed_steps"][2]["transferred_length"], 6);
    assert!(json["executed_steps"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .all(|(index, step)| index == 2 || step["transferred_length"] == 64));
    assert_eq!(json["target_count"], 1);
    assert_eq!(json["set_color_frames"], 1);
    assert_eq!(json["canonical_targets"], serde_json::json!(["f1"]));
    assert_eq!(json["state_restore_available"], false);
    assert_eq!(json["persistence"], "not_claimed");
}

#[test]
fn keyboard_live_accepts_sixteen_explicit_keys_in_two_color_frames() {
    const SIXTEEN: &str = "escape,f1,f2,f3,f4,f5,f6,f7,f8,f9,f10,f11,f12,home,end,delete";
    let state = SharedState::default();
    let mut chassis = FakeExecutor::success(state.clone());
    let mut keyboard = FakeKeyboardExecutor::success(state.clone());
    let mut warnings = FakeWarnings::new(state);
    let cli = parse(strings(&[
        "set",
        "--device",
        "keyboard",
        "--target",
        SIXTEEN,
        "--color",
        "ff0000",
        "--apply",
        "--experimental",
        "--confirm-live-write",
        "--json",
    ]))
    .unwrap();
    let output = run_with_all_services(
        cli,
        supported_keyboard_inventory(),
        &mut chassis,
        &mut keyboard,
        &mut warnings,
    )
    .unwrap();
    let json: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(keyboard.calls, 1);
    assert_eq!(keyboard.intents[0].targets.len(), 16);
    assert_eq!(
        keyboard.intents[0]
            .targets
            .iter()
            .map(|target| target.logical_id)
            .collect::<Vec<_>>(),
        (0..16).collect::<Vec<_>>()
    );
    assert_eq!(json["set_color_frames"], 2);
    assert_eq!(json["executed_steps"].as_array().unwrap().len(), 7);
}

#[test]
fn keyboard_live_accepts_fifteen_nonnumeric_keys_in_one_color_frame() {
    const FIFTEEN: &str = "escape,f1,f2,f3,f4,f5,f6,f7,f8,f9,f10,f11,f12,home,end";
    assert!(FIFTEEN
        .split(',')
        .all(|target| !target.chars().all(|character| character.is_ascii_digit())));

    let state = SharedState::default();
    let mut chassis = FakeExecutor::success(state.clone());
    let mut keyboard = FakeKeyboardExecutor::success(state.clone());
    let mut warnings = FakeWarnings::new(state);
    let cli = parse(strings(&[
        "set",
        "--device",
        "keyboard",
        "--target",
        FIFTEEN,
        "--color",
        "ff0000",
        "--apply",
        "--experimental",
        "--confirm-live-write",
        "--json",
    ]))
    .unwrap();
    let output = run_with_all_services(
        cli,
        supported_keyboard_inventory(),
        &mut chassis,
        &mut keyboard,
        &mut warnings,
    )
    .unwrap();
    let json: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(keyboard.calls, 1);
    assert_eq!(keyboard.intents[0].targets.len(), 15);
    assert_eq!(json["set_color_frames"], 1);
    assert_eq!(json["executed_steps"].as_array().unwrap().len(), 6);
}

#[test]
fn keyboard_live_accepts_all_seventy_five_explicit_nonnumeric_catalog_names() {
    let names = keyboard_targets()
        .iter()
        .filter(|target| {
            !target
                .name
                .chars()
                .all(|character| character.is_ascii_digit())
        })
        .map(|target| target.name)
        .collect::<Vec<_>>();
    assert_eq!(names.len(), 75);
    let target_list = names.join(",");
    let state = SharedState::default();
    let mut chassis = FakeExecutor::success(state.clone());
    let mut keyboard = FakeKeyboardExecutor::success(state.clone());
    let mut warnings = FakeWarnings::new(state);
    let cli = parse(strings(&[
        "set",
        "--device",
        "keyboard",
        "--target",
        &target_list,
        "--color",
        "ff0000",
        "--apply",
        "--experimental",
        "--confirm-live-write",
        "--json",
    ]))
    .unwrap();
    let output = run_with_all_services(
        cli,
        supported_keyboard_inventory(),
        &mut chassis,
        &mut keyboard,
        &mut warnings,
    )
    .unwrap();
    let json: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(keyboard.calls, 1);
    assert_eq!(json["target_count"], 75);
    assert_eq!(json["set_color_frames"], 5);
    assert_eq!(json["executed_steps"].as_array().unwrap().len(), 10);
    assert!(json["canonical_targets"]
        .as_array()
        .unwrap()
        .iter()
        .all(|target| !target
            .as_str()
            .unwrap()
            .chars()
            .all(|character| character.is_ascii_digit())));
}

#[test]
fn keyboard_live_explicit_catalog_boundary_is_seventy_five_before_name_resolution() {
    let mut names = keyboard_targets()
        .iter()
        .filter(|target| {
            !target
                .name
                .chars()
                .all(|character| character.is_ascii_digit())
        })
        .map(|target| target.name)
        .collect::<Vec<_>>();
    assert_eq!(names.len(), 75);
    names.push("unknown-over-bound");
    let target_list = names.join(",");
    let error = parse(strings(&[
        "set",
        "--device",
        "keyboard",
        "--target",
        &target_list,
        "--color",
        "ff0000",
        "--apply",
        "--experimental",
        "--confirm-live-write",
    ]))
    .unwrap_err();
    assert_eq!(
        error,
        "keyboard live apply accepts at most 75 explicit nonnumeric key names"
    );
}

#[test]
fn keyboard_live_all_expands_exact_catalog_once_and_suppresses_huge_human_line() {
    let state = SharedState::default();
    let mut chassis = FakeExecutor::success(state.clone());
    let mut keyboard = FakeKeyboardExecutor::success(state.clone());
    let mut warnings = FakeWarnings::new(state);
    let cli = parse(strings(&[
        "set",
        "--device",
        "keyboard",
        "--target",
        "all",
        "--color",
        "ff0000",
        "--apply",
        "--experimental",
        "--confirm-live-write",
        "--json",
    ]))
    .unwrap();
    let output = run_with_all_services(
        cli,
        supported_keyboard_inventory(),
        &mut chassis,
        &mut keyboard,
        &mut warnings,
    )
    .unwrap();
    let json: serde_json::Value = serde_json::from_str(&output).unwrap();
    let expected = keyboard_targets();
    assert_eq!(keyboard.calls, 1);
    assert_eq!(keyboard.intents[0].targets.len(), 85);
    assert_eq!(
        keyboard.intents[0]
            .targets
            .iter()
            .map(|target| (target.target, target.logical_id))
            .collect::<Vec<_>>(),
        expected
            .iter()
            .map(|target| (target.name, target.logical_id))
            .collect::<Vec<_>>()
    );
    assert!(keyboard.intents[0]
        .targets
        .iter()
        .any(|target| target.target == "1" && target.logical_id == 21));
    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["target_count"], 85);
    assert_eq!(json["set_color_frames"], 6);
    assert_eq!(json["executed_steps"].as_array().unwrap().len(), 11);
    assert_eq!(json["canonical_targets"].as_array().unwrap().len(), 85);

    let state = SharedState::default();
    let mut chassis = FakeExecutor::success(state.clone());
    let mut keyboard = FakeKeyboardExecutor::success(state.clone());
    let mut warnings = FakeWarnings::new(state);
    let cli = parse(strings(&[
        "set",
        "--device",
        "keyboard",
        "--target",
        "all",
        "--color",
        "ff0000",
        "--apply",
        "--experimental",
        "--confirm-live-write",
    ]))
    .unwrap();
    let human = run_with_all_services(
        cli,
        supported_keyboard_inventory(),
        &mut chassis,
        &mut keyboard,
        &mut warnings,
    )
    .unwrap();
    assert!(human.contains("all known keyboard targets (count 85)"));
    assert!(!human.contains("escape,f1,f2,f3"));
    assert!(warnings.messages[0].contains("set_color_frames=6"));
    assert!(warnings.messages[0].contains("HID operations=11"));
    assert!(warnings.messages[0].contains("about 55 seconds"));
    assert!(warnings.messages[0].contains("partial applied state"));
}

#[test]
fn keyboard_live_empty_target_item_is_rejected_by_parser() {
    assert!(parse(strings(&[
        "set",
        "--device",
        "keyboard",
        "--target",
        "f1,,f2",
        "--color",
        "ff0000",
        "--apply",
        "--experimental",
        "--confirm-live-write",
    ]))
    .is_err());
}

#[test]
fn invalid_keyboard_live_targets_never_call_executor() {
    for targets in [
        vec!["f1,all"],
        vec!["all,f1"],
        vec!["all", "all"],
        vec!["escape", "esc"],
        vec!["f1", "f2"],
        vec!["esc,ESC"],
        vec!["1"],
        vec!["unknown"],
    ] {
        let state = SharedState::default();
        let mut chassis = FakeExecutor::success(state.clone());
        let mut keyboard = FakeKeyboardExecutor::success(state.clone());
        let mut warnings = FakeWarnings::new(state);
        let mut args = vec!["set", "--device", "keyboard"];
        for target in targets {
            args.extend(["--target", target]);
        }
        args.extend([
            "--color",
            "abcdef",
            "--apply",
            "--experimental",
            "--confirm-live-write",
        ]);
        if let Ok(cli) = parse(args.into_iter().map(str::to_string)) {
            assert!(run_with_all_services(
                cli,
                supported_keyboard_inventory(),
                &mut chassis,
                &mut keyboard,
                &mut warnings,
            )
            .is_err());
        }
        assert_eq!(keyboard.calls, 0);
        assert!(warnings.messages.is_empty());
    }
}

#[test]
fn invalid_chassis_live_targets_never_call_executor() {
    for target_args in [vec!["back", "chassis"], vec!["1"], vec!["unknown"]] {
        let state = SharedState::default();
        let mut executor = FakeExecutor::success(state.clone());
        let mut warnings = FakeWarnings::new(state);
        let mut args = vec!["set", "--device", "chassis"];
        for target in target_args {
            args.extend(["--target", target]);
        }
        args.extend([
            "--color",
            "abcdef",
            "--apply",
            "--experimental",
            "--confirm-live-write",
        ]);
        let cli = parse(args.into_iter().map(str::to_string)).unwrap();
        assert!(
            run_with_services(cli, supported_inventory(), &mut executor, &mut warnings).is_err()
        );
        assert_eq!(executor.calls, 0);
    }
}

#[test]
fn valid_live_intent_warns_before_exactly_one_typed_execution() {
    let state = SharedState::default();
    let mut executor = FakeExecutor::success(state.clone());
    let mut warnings = FakeWarnings::new(state.clone());
    let cli = parse(strings(&[
        "set",
        "--device",
        "chassis",
        "--target",
        "chassis",
        "--color",
        "Aa00Ff",
        "--apply",
        "--experimental",
        "--confirm-live-write",
    ]))
    .unwrap();
    let output =
        run_with_services(cli, supported_inventory(), &mut executor, &mut warnings).unwrap();
    assert_eq!(executor.calls, 1);
    assert_eq!(executor.intents[0].target, "back");
    assert_eq!(executor.intents[0].logical_id, 2);
    assert_eq!(executor.intents[0].color.hex(), "#aa00ff");
    assert_eq!(state.borrow().events, vec!["warning", "execute"]);
    assert!(warnings.messages[0].contains("volatile static write"));
    assert!(warnings.messages[0].contains("no reliable color readback"));
    assert!(warnings.messages[0].contains("target=back"));
    assert!(warnings.messages[0].contains("color=#aa00ff"));
    assert!(output.starts_with("EXPERIMENTAL AW-ELC WRITE COMPLETED"));
    assert!(output.contains("state restore available: no"));
}

#[test]
fn live_success_json_is_structured_and_has_no_warning_stdout() {
    let state = SharedState::default();
    let mut executor = FakeExecutor::success(state.clone());
    let mut warnings = FakeWarnings::new(state);
    let cli = parse(strings(&[
        "set",
        "--device",
        "chassis",
        "--target",
        "haptic",
        "--color",
        "010203",
        "--apply",
        "--experimental",
        "--confirm-live-write",
        "--json",
    ]))
    .unwrap();
    let output =
        run_with_services(cli, supported_inventory(), &mut executor, &mut warnings).unwrap();
    let json: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(warnings.messages.len(), 0);
    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["command"], "set");
    assert_eq!(json["mode"], "apply");
    assert_eq!(json["transport_performed"], true);
    assert_eq!(json["requested_target"], "haptic");
    assert_eq!(json["resolved_target"], "touchpad");
    assert_eq!(json["canonical_targets"], serde_json::json!(["touchpad"]));
    assert_eq!(json["target_count"], 1);
    assert_eq!(json["set_color_frames"], 1);
    assert_eq!(json["color"], "#010203");
    assert_eq!(json["controller_vid"], "187c");
    assert_eq!(json["controller_pid"], "0551");
    assert_eq!(json["executed_steps"][0]["name"], "remove");
    assert_eq!(json["executed_steps"][0]["transferred_length"], 33);
    assert_eq!(json["state_restore_available"], false);
    assert_eq!(json["persistence"], "not_claimed");
}

#[test]
fn executor_failure_returns_only_failure_and_never_success() {
    let state = SharedState::default();
    let mut executor = FakeExecutor::failure(state.clone());
    let mut warnings = FakeWarnings::new(state);
    let cli = parse(strings(&[
        "set",
        "--device",
        "chassis",
        "--target",
        "back",
        "--color",
        "ffffff",
        "--apply",
        "--experimental",
        "--confirm-live-write",
        "--json",
    ]))
    .unwrap();
    let error =
        run_with_services(cli, supported_inventory(), &mut executor, &mut warnings).unwrap_err();
    assert_eq!(executor.calls, 1);
    assert_eq!(error.code, "transport_failed");
    assert!(!error.transport_performed);
    let json = serde_json::to_value(error).unwrap();
    assert_eq!(json["transport_performed"], false);
}

#[test]
fn keyboard_status_service_captures_six_bytes_with_warning_separation() {
    let state = SharedState::default();
    let mut executor =
        FakeStatusExecutor::success(state.clone(), vec![0xcc, 0x01, 0x80, 0x02, 0xaa, 0xff]);
    let mut warnings = FakeWarnings::new(state.clone());
    let human = run_keyboard_status_with_services(false, &mut executor, &mut warnings).unwrap();
    assert_eq!(executor.calls, 1);
    assert_eq!(state.borrow().events, ["warning", "status_execute"]);
    assert!(warnings.messages[0].contains("exactly one 64-byte"));
    assert!(warnings.messages[0].contains("no reset, color_set, loop, or update"));
    assert!(human.contains("response hex: cc018002aaff"));
    assert!(human.contains("not RGB readback"));

    let state = SharedState::default();
    let mut executor = FakeStatusExecutor::success(state.clone(), vec![0xcc, 0x8c]);
    let mut warnings = FakeWarnings::new(state.clone());
    let output = run_keyboard_status_with_services(true, &mut executor, &mut warnings).unwrap();
    let json: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert!(warnings.messages.is_empty());
    assert_eq!(json["transport_performed"], true);
    assert_eq!(json["color_frames_sent"], false);
    assert_eq!(json["query_write_length"], 64);
    assert_eq!(json["response_length"], 2);
    assert_eq!(json["response_hex"], "cc8c");
    assert_eq!(json["color_readback"], "not_performed_status_bytes_only");
}

#[test]
fn keyboard_status_service_failure_is_non_success_and_not_retried() {
    let state = SharedState::default();
    let mut executor = FakeStatusExecutor::failure(state.clone());
    let mut warnings = FakeWarnings::new(state);
    let error = run_keyboard_status_with_services(true, &mut executor, &mut warnings).unwrap_err();
    assert_eq!(executor.calls, 1);
    assert!(!error.transport_performed);
}

#[derive(Clone, Default)]
struct SharedState(Rc<RefCell<State>>);

impl std::ops::Deref for SharedState {
    type Target = Rc<RefCell<State>>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Default)]
struct State {
    events: Vec<&'static str>,
}

struct FakePowerProfileExecutor {
    state: SharedState,
    calls: usize,
    colors: Vec<Rgb>,
}

impl FakePowerProfileExecutor {
    fn success(state: SharedState) -> Self {
        Self {
            state,
            calls: 0,
            colors: Vec::new(),
        }
    }
}

impl LivePowerProfileExecutor for FakePowerProfileExecutor {
    fn execute(
        &mut self,
        intent: PowerProfileLiveIntent,
    ) -> Result<LiveExecutionReceipt, CliRunError> {
        self.calls += 1;
        self.colors.push(intent.color);
        self.state.borrow_mut().events.push("power_execute");
        Ok(LiveExecutionReceipt {
            steps: crate::protocol::power_v4::encode_equal_color(intent.color)
                .steps()
                .iter()
                .map(|step| LiveExecutedStep {
                    name: step.name(),
                    transferred_length: 33,
                })
                .collect(),
        })
    }
}

struct FakeExecutor {
    state: SharedState,
    calls: usize,
    intents: Vec<ChassisLiveIntent>,
    fail: bool,
}

impl FakeExecutor {
    fn success(state: SharedState) -> Self {
        Self {
            state,
            calls: 0,
            intents: Vec::new(),
            fail: false,
        }
    }

    fn failure(state: SharedState) -> Self {
        Self {
            fail: true,
            ..Self::success(state)
        }
    }
}

impl LiveChassisExecutor for FakeExecutor {
    fn execute(&mut self, intent: ChassisLiveIntent) -> Result<LiveExecutionReceipt, CliRunError> {
        self.calls += 1;
        self.intents.push(intent);
        self.state.borrow_mut().events.push("execute");
        if self.fail {
            Err(CliRunError::transport("fake transport failure"))
        } else {
            Ok(LiveExecutionReceipt {
                steps: vec![
                    LiveExecutedStep {
                        name: "remove",
                        transferred_length: 33,
                    },
                    LiveExecutedStep {
                        name: "start",
                        transferred_length: 33,
                    },
                    LiveExecutedStep {
                        name: "set_color",
                        transferred_length: 33,
                    },
                    LiveExecutedStep {
                        name: "finish_play",
                        transferred_length: 33,
                    },
                ],
            })
        }
    }
}

struct FakeKeyboardExecutor {
    state: SharedState,
    calls: usize,
    intents: Vec<KeyboardLiveIntent>,
}

impl FakeKeyboardExecutor {
    fn success(state: SharedState) -> Self {
        Self {
            state,
            calls: 0,
            intents: Vec::new(),
        }
    }
}

impl LiveKeyboardExecutor for FakeKeyboardExecutor {
    fn execute(&mut self, intent: KeyboardLiveIntent) -> Result<LiveExecutionReceipt, CliRunError> {
        self.calls += 1;
        let frame_count = intent.targets.len().div_ceil(15);
        self.intents.push(intent);
        self.state.borrow_mut().events.push("keyboard_execute");
        let mut steps = vec![
            LiveExecutedStep {
                name: "reset",
                transferred_length: 64,
            },
            LiveExecutedStep {
                name: "query_status",
                transferred_length: 64,
            },
            LiveExecutedStep {
                name: "read_status",
                transferred_length: 6,
            },
        ];
        steps.extend((0..frame_count).map(|_| LiveExecutedStep {
            name: "set_color",
            transferred_length: 64,
        }));
        steps.extend([
            LiveExecutedStep {
                name: "loop",
                transferred_length: 64,
            },
            LiveExecutedStep {
                name: "update",
                transferred_length: 64,
            },
        ]);
        Ok(LiveExecutionReceipt { steps })
    }
}

struct FakeStatusExecutor {
    state: SharedState,
    calls: usize,
    response: Vec<u8>,
    fail: bool,
}

impl FakeStatusExecutor {
    fn success(state: SharedState, response: Vec<u8>) -> Self {
        Self {
            state,
            calls: 0,
            response,
            fail: false,
        }
    }

    fn failure(state: SharedState) -> Self {
        Self {
            fail: true,
            ..Self::success(state, Vec::new())
        }
    }
}

impl KeyboardStatusExecutor for FakeStatusExecutor {
    fn execute(&mut self) -> Result<KeyboardStatusReceipt, CliRunError> {
        self.calls += 1;
        self.state.borrow_mut().events.push("status_execute");
        if self.fail {
            Err(CliRunError::transport("fake status failure"))
        } else {
            Ok(KeyboardStatusReceipt {
                query_write_length: 64,
                response: self.response.clone(),
            })
        }
    }
}

struct FakeWarnings {
    state: SharedState,
    messages: Vec<String>,
}

impl FakeWarnings {
    fn new(state: SharedState) -> Self {
        Self {
            state,
            messages: Vec::new(),
        }
    }
}

impl WarningSink for FakeWarnings {
    fn warn(&mut self, message: &str) {
        self.state.borrow_mut().events.push("warning");
        self.messages.push(message.to_string());
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
        vec![DeviceSummary {
            kind: DeviceKind::Chassis,
            status: DeviceStatus::Found,
            vid: "187c".into(),
            pid: "0551".into(),
            manufacturer: None,
            product: None,
            serial: None,
            sysfs_name: Some("3-2".into()),
            bus_number: Some(3),
            port_path: Some(vec![2]),
            interface_number: Some("00".into()),
            hidraw: Vec::new(),
            descriptor: None,
        }],
    )
}

fn supported_keyboard_inventory() -> (DmiIdentity, Vec<DeviceSummary>) {
    let (dmi, mut devices) = supported_inventory();
    devices.push(DeviceSummary {
        kind: DeviceKind::Keyboard,
        status: DeviceStatus::Found,
        vid: "0d62".into(),
        pid: "d2b1".into(),
        manufacturer: Some("DELL Technologies".into()),
        product: Some("Keyboard".into()),
        serial: None,
        sysfs_name: Some("3-2".into()),
        bus_number: Some(3),
        port_path: Some(vec![2]),
        interface_number: Some("00".into()),
        hidraw: Vec::new(),
        descriptor: Some(DescriptorStatus {
            evidence: DescriptorEvidence::HashMatch,
            sha256: Some(KEYBOARD_DESCRIPTOR_SHA256.into()),
            expected_sha256: KEYBOARD_DESCRIPTOR_SHA256,
            interface_number: Some("00".into()),
            sysfs_path: Some("/sys/fake/report_descriptor".into()),
        }),
    });
    (dmi, devices)
}

#[test]
fn set_all_parser_and_dry_run_report_are_exact() {
    let cli = parse(strings(&[
        "set-all",
        "--color",
        "#FF69B4",
        "--dry-run",
        "--json",
    ]))
    .unwrap();
    assert!(
        matches!(cli.command, CliCommand::SetAll { color, dry_run: true, apply: false, .. } if color.hex() == "#ff69b4")
    );
    let (dmi, devices) = supported_keyboard_inventory();
    let output = run_with_inventory(cli, dmi, devices).unwrap();
    let json: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(json["command"], "set_all");
    assert_eq!(json["transport_attempted"], false);
    assert_eq!(json["transport_performed"], false);
    assert_eq!(json["color"], "#ff69b4");
    assert_eq!(json["validation"], "exact_set_all_hot_pink_live_validated");
    let evidence = json["validation_evidence"].as_str().unwrap();
    for text in [
        "Alienware m16 R2 BIOS 1.21.0",
        "keyboard 0d62:d2b1",
        "AW-ELC 187c:0551",
        "keyboard 11/11",
        "combined touchpad/back 4/4",
        "power profile 34/34",
        "49/49 total",
        "no retry",
        "visibly confirmed keyboard, touchpad, rear, and power Hot Pink",
        "AC online=1",
        "BAT0 Charging",
        "capacity=36%",
        "only current AC-charging state",
        "Non-atomic",
        "no rollback/readback/restore",
        "power persistence unknown",
        "Other colors, power states, and individual keyboard ID identity beyond existing records remain unvalidated",
    ] {
        assert!(evidence.contains(text), "missing {text:?} in {evidence}");
    }
    assert_eq!(json["keyboard"]["assignment_count"], 85);
    assert_eq!(json["keyboard"]["color_frame_count"], 6);
    assert_eq!(json["keyboard"]["step_count"], 11);
    assert_eq!(
        json["aw_static_touchpad_back"]["logical_ids"],
        serde_json::json!([0, 2])
    );
    assert_eq!(json["aw_static_touchpad_back"]["step_count"], 4);
    assert_eq!(json["power_profile"]["state_count"], 6);
    assert_eq!(json["power_profile"]["step_count"], 34);
    assert_eq!(json["total_transport_steps"], 49);
    assert!(json["keyboard"]["plan"]["assignments"]
        .as_array()
        .unwrap()
        .iter()
        .all(|a| a["color_hex"] == "#ff69b4" && a["validation"] == "mapping_derived_unvalidated"));
    assert!(json["aw_static_touchpad_back"]["plan"]["assignments"]
        .as_array()
        .unwrap()
        .iter()
        .all(|a| a["color_hex"] == "#ff69b4" && a["validation"] == "mapping_derived_unvalidated"));
    assert_eq!(
        json["power_profile"]["plan"]["validation"],
        "mapping_derived_unvalidated"
    );

    let human_cli = parse(strings(&["set-all", "--color", "FF69B4", "--dry-run"])).unwrap();
    let human = run_with_inventory(human_cli, empty_dmi(), Vec::new()).unwrap();
    assert!(human.contains("validation=ExactSetAllHotPinkLiveValidated"));
    assert!(human.contains("49/49"));
    assert!(human.contains("current AC-charging state only"));
    assert!(!human.contains("all six power states"));
    assert!(!human.contains("all keyboard IDs"));
}

#[test]
fn set_all_other_color_stays_globally_unvalidated_without_changing_plan_validation() {
    let cli = parse(strings(&[
        "set-all",
        "--color",
        "00ff00",
        "--dry-run",
        "--json",
    ]))
    .unwrap();
    let output = run_with_inventory(cli, empty_dmi(), Vec::new()).unwrap();
    let json: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(json["validation"], "mapping_derived_unvalidated");
    assert!(json["validation_evidence"]
        .as_str()
        .unwrap()
        .contains("No exact global set-all live validation exists for #00ff00"));
    for stage in ["keyboard", "aw_static_touchpad_back", "power_profile"] {
        assert_eq!(
            json[stage]["plan"]["validation"],
            "mapping_derived_unvalidated"
        );
        assert!(json[stage]["plan"]["assignments"]
            .as_array()
            .unwrap()
            .iter()
            .all(|assignment| assignment["validation"] == "mapping_derived_unvalidated"));
    }
}

#[test]
fn set_all_live_success_warns_once_and_partial_reports_are_structured() {
    let state = SharedState::default();
    let mut executor = FakeSetAllExecutor {
        state: state.clone(),
        calls: 0,
        outcome: SetAllExecutionOutcome::successful(),
    };
    let mut warnings = FakeWarnings::new(state.clone());
    let cli = parse(strings(&[
        "set-all",
        "--color",
        "ff69b4",
        "--apply",
        "--experimental",
        "--confirm-live-write",
        "--confirm-power-profile-write",
        "--confirm-set-all-write",
        "--json",
    ]))
    .unwrap();
    let output = run_set_all_with_services(cli, &mut executor, &mut warnings).unwrap();
    let json: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(executor.calls, 1);
    assert_eq!(state.borrow().events, ["warning", "set_all_execute"]);
    assert_eq!(json["overall_status"], "completed");
    assert_eq!(json["validation"], "exact_set_all_hot_pink_live_validated");
    assert!(json["validation_evidence"]
        .as_str()
        .unwrap()
        .contains("49/49 total"));
    assert_eq!(json["transport_attempted"], true);
    assert_eq!(json["transport_performed"], true);
    assert_eq!(json["stages"][0]["transport_attempted"], true);
    assert_eq!(json["stages"][0]["completed_steps"], 11);
    assert_eq!(json["stages"][1]["completed_steps"], 4);
    assert_eq!(json["stages"][2]["completed_steps"], 34);
    assert_eq!(json["total_transport_steps"], 49);
    for text in [
        "two controllers",
        "four RGB groups",
        "fixed order",
        "49 transport operations",
        "about 74 seconds",
        "non-atomic partial-state",
        "persistence unknown",
        "no rollback/readback/restore/retry",
    ] {
        assert!(warnings.messages[0].contains(text), "{text}");
    }

    for (stage, completed) in [(0, 0), (0, 3), (1, 0), (1, 2), (2, 0), (2, 17), (2, 33)] {
        let state = SharedState::default();
        let mut executor = FakeSetAllExecutor {
            state: state.clone(),
            calls: 0,
            outcome: SetAllExecutionOutcome::failed_for_test(stage, completed),
        };
        let mut warnings = FakeWarnings::new(state);
        let cli = parse(strings(&[
            "set-all",
            "--color",
            "ff69b4",
            "--apply",
            "--experimental",
            "--confirm-live-write",
            "--confirm-power-profile-write",
            "--confirm-set-all-write",
            "--json",
        ]))
        .unwrap();
        let error = run_set_all_with_services(cli, &mut executor, &mut warnings).unwrap_err();
        assert_eq!(executor.calls, 1);
        assert_eq!(error.code, "set_all_partial_failure");
        let report = error.report.unwrap();
        assert_eq!(report.overall_status, Some(CompoundStatus::PartialFailure));
        assert!(report.transport_attempted);
        assert!(report.transport_performed);
        assert_eq!(report.stages[stage].status, CompoundStageStatus::Failed);
        assert!(report.stages[stage].transport_attempted);
        assert_eq!(report.stages[stage].completed_steps, completed);
        assert!(report
            .stages
            .iter()
            .skip(stage + 1)
            .all(|record| record.status == CompoundStageStatus::NotStarted));
        assert_eq!(report.rollback, "not_attempted_not_available");
    }
}

#[test]
fn set_all_confirmation_matrix_is_strict() {
    let required = [
        "--apply",
        "--experimental",
        "--confirm-live-write",
        "--confirm-power-profile-write",
        "--confirm-set-all-write",
    ];
    for missing in 0..required.len() {
        let mut args = vec!["set-all", "--color", "ff69b4"];
        args.extend(
            required
                .iter()
                .enumerate()
                .filter_map(|(i, flag)| (i != missing).then_some(*flag)),
        );
        assert!(
            parse(args.into_iter().map(str::to_string)).is_err(),
            "missing {}",
            required[missing]
        );
    }
    for flag in [
        "--experimental",
        "--confirm-live-write",
        "--confirm-power-profile-write",
        "--confirm-set-all-write",
    ] {
        assert!(parse(strings(&[
            "set-all",
            "--color",
            "ff69b4",
            "--dry-run",
            flag
        ]))
        .is_err());
    }
    assert!(parse(strings(&[
        "set-all",
        "--color",
        "#ff69b4",
        "--apply",
        "--experimental",
        "--confirm-live-write",
        "--confirm-power-profile-write",
        "--confirm-set-all-write"
    ]))
    .is_err());
}

#[test]
fn set_all_state_is_saved_only_after_complete_live_success() {
    let state = SharedState::default();
    let mut executor = FakeSetAllExecutor {
        state: state.clone(),
        calls: 0,
        outcome: SetAllExecutionOutcome::successful(),
    };
    let mut warnings = FakeWarnings::new(state.clone());
    let mut store = FakeSetAllStateStore::new(state.clone());
    let dry_run = parse(strings(&["set-all", "--color", "Aa00Ff", "--dry-run"])).unwrap();
    run_set_all_with_services_and_state(dry_run, &mut executor, &mut warnings, &mut store).unwrap();
    assert_eq!(executor.calls, 0);
    assert!(store.colors.is_empty());

    let mut executor = FakeSetAllExecutor {
        state: state.clone(),
        calls: 0,
        outcome: SetAllExecutionOutcome::failed_for_test(2, 33),
    };
    let apply = set_all_apply_cli("aa00ff");
    let error =
        run_set_all_with_services_and_state(apply, &mut executor, &mut warnings, &mut store)
            .unwrap_err();
    assert_eq!(error.code, "set_all_partial_failure");
    assert!(store.colors.is_empty());

    let mut executor = FakeSetAllExecutor {
        state: state.clone(),
        calls: 0,
        outcome: SetAllExecutionOutcome { stages: Vec::new() },
    };
    let error = run_set_all_with_services_and_state(
        set_all_apply_cli("aa00ff"),
        &mut executor,
        &mut warnings,
        &mut store,
    )
    .unwrap_err();
    assert_eq!(error.code, "incomplete_set_all_execution");
    assert!(store.colors.is_empty());

    let mut executor = FakeSetAllExecutor {
        state: state.clone(),
        calls: 0,
        outcome: SetAllExecutionOutcome::successful(),
    };
    run_set_all_with_services_and_state(
        set_all_apply_cli("Aa00Ff"),
        &mut executor,
        &mut warnings,
        &mut store,
    )
    .unwrap();
    assert_eq!(store.colors, [Rgb::new(0xaa, 0x00, 0xff)]);
    assert_eq!(
        state.borrow().events.last(),
        Some(&"state_persist"),
        "state must be persisted only after the transport executor completes"
    );
}

#[test]
fn resume_helper_matches_only_the_complete_ready_for_writes_json_line() {
    let helper = include_str!("../../contrib/systemd/alienrgb-resume");
    assert!(helper.contains(
        "grep -Eq '^[[:space:]]*\"ready_for_writes\"[[:space:]]*:[[:space:]]*true[[:space:]]*,?[[:space:]]*$'"
    ));
    assert!(!helper.contains("grep -Eq '\"ready_for_writes\"[[:space:]]*:[[:space:]]*true'"));
}

#[test]
fn set_all_success_reports_state_persistence_failure_explicitly() {
    let state = SharedState::default();
    let mut executor = FakeSetAllExecutor {
        state: state.clone(),
        calls: 0,
        outcome: SetAllExecutionOutcome::successful(),
    };
    let mut warnings = FakeWarnings::new(state.clone());
    let mut store = FakeSetAllStateStore::new(state);
    store.fail = true;

    let error = run_set_all_with_services_and_state(
        set_all_apply_cli("010203"),
        &mut executor,
        &mut warnings,
        &mut store,
    )
    .unwrap_err();

    assert_eq!(executor.calls, 1);
    assert_eq!(store.calls, 1);
    assert_eq!(error.code, "resume_state_persist_failed");
    assert!(error.transport_performed);
    assert!(error.message.contains("last successful set-all state"));
}

fn set_all_apply_cli(color: &str) -> Cli {
    parse(strings(&[
        "set-all",
        "--color",
        color,
        "--apply",
        "--experimental",
        "--confirm-live-write",
        "--confirm-power-profile-write",
        "--confirm-set-all-write",
        "--json",
    ]))
    .unwrap()
}

struct FakeSetAllStateStore {
    state: SharedState,
    calls: usize,
    colors: Vec<Rgb>,
    fail: bool,
}

impl FakeSetAllStateStore {
    fn new(state: SharedState) -> Self {
        Self {
            state,
            calls: 0,
            colors: Vec::new(),
            fail: false,
        }
    }
}

impl crate::resume_state::SetAllStateStore for FakeSetAllStateStore {
    fn persist_color(&mut self, color: Rgb) -> std::io::Result<()> {
        self.calls += 1;
        self.colors.push(color);
        self.state.borrow_mut().events.push("state_persist");
        if self.fail {
            Err(std::io::Error::other("forced state failure"))
        } else {
            Ok(())
        }
    }
}

struct FakeSetAllExecutor {
    state: SharedState,
    calls: usize,
    outcome: SetAllExecutionOutcome,
}

impl LiveSetAllExecutor for FakeSetAllExecutor {
    fn execute(
        &mut self,
        _intent: SetAllLiveIntent,
    ) -> Result<SetAllExecutionOutcome, CliRunError> {
        self.calls += 1;
        self.state.borrow_mut().events.push("set_all_execute");
        Ok(self.outcome.clone())
    }
}

fn strings<'a>(values: &'a [&'a str]) -> impl Iterator<Item = String> + 'a {
    values.iter().map(|value| (*value).to_string())
}

#[test]
fn pre_login_boot_restore_contract_uses_the_unmodified_helper_and_group_access() {
    let boot_unit = include_str!("../../contrib/systemd/alienrgb-boot@.service");
    for directive in [
        "User=%i",
        "SupplementaryGroups=alienrgb",
        "Type=oneshot",
        "ExecStart=/usr/local/libexec/alienrgb-resume",
        "TimeoutStartSec=120s",
        "Before=display-manager.service",
        "WantedBy=graphical.target",
    ] {
        assert!(boot_unit.contains(directive), "missing {directive:?}");
    }

    let resume_unit = include_str!("../../contrib/systemd/alienrgb-resume@.service");
    assert!(resume_unit.contains("SupplementaryGroups=alienrgb"));

    let rules = include_str!("../../contrib/udev/70-alienrgb.rules");
    let matches = rules
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect::<Vec<_>>();
    assert_eq!(matches.len(), 2);
    for rule in &matches {
        assert!(
            rule.contains("GROUP:=\"alienrgb\""),
            "missing group: {rule}"
        );
        assert!(rule.contains("MODE:=\"0660\""), "missing mode: {rule}");
        assert!(
            rule.contains("TAG+=\"uaccess\""),
            "missing uaccess tag: {rule}"
        );
    }
    assert!(matches[0].contains("ENV{ID_USB_INTERFACE_NUM}==\"00\""));
    assert!(matches[1].contains("ENV{ID_USB_INTERFACES}==\"*:030000:*\""));
}
