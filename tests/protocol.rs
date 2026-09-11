use alienrgb::model::{ProtocolFamily, TransferKind, ValidationState};
use alienrgb::protocol::{api_v4, api_v5, power_v4, EncodeError, LogicalColor, Rgb};
use alienrgb::targets::{
    expand_chassis_targets, expand_keyboard_targets, keyboard_targets, lookup_chassis_target,
    lookup_keyboard_target, TargetError,
};

#[test]
fn v5_emits_byte_exact_primary_color_records_and_static_sequence() {
    let plan = api_v5::encode_static(&[
        LogicalColor::new(0, Rgb::new(255, 0, 0)),
        LogicalColor::new(1, Rgb::new(0, 255, 0)),
        LogicalColor::new(2, Rgb::new(0, 0, 255)),
    ])
    .unwrap();
    let steps = plan.steps();

    assert_eq!(plan.family(), ProtocolFamily::AlienFxApiV5);
    assert_eq!(steps.len(), 6);
    assert_step(
        &steps[0],
        TransferKind::HidFeatureWrite,
        64,
        64,
        &[0xcc, 0x94],
    );
    assert_step(
        &steps[1],
        TransferKind::HidFeatureWrite,
        64,
        64,
        &[0xcc, 0x93],
    );
    assert_step(
        &steps[2],
        TransferKind::HidFeatureReadIntent,
        64,
        64,
        &[0xcc],
    );
    assert_step(
        &steps[3],
        TransferKind::HidFeatureWrite,
        64,
        64,
        &[
            0xcc, 0x8c, 0x02, 0x00, 0x01, 0xff, 0x00, 0x00, 0x02, 0x00, 0xff, 0x00, 0x03, 0x00,
            0x00, 0xff,
        ],
    );
    assert_step(
        &steps[4],
        TransferKind::HidFeatureWrite,
        64,
        64,
        &[0xcc, 0x8c, 0x13],
    );
    assert_step(
        &steps[5],
        TransferKind::HidFeatureWrite,
        64,
        64,
        &[0xcc, 0x8b, 0x01, 0xff],
    );
}

#[test]
fn v5_batches_sixteen_keys_as_fifteen_plus_one() {
    let assignments = keyboard_targets()[..16]
        .iter()
        .map(|target| LogicalColor::new(target.logical_id, Rgb::new(1, 2, 3)))
        .collect::<Vec<_>>();
    let plan = api_v5::encode_static(&assignments).unwrap();
    let frames = plan
        .steps()
        .iter()
        .filter(|step| step.name() == "set_color")
        .collect::<Vec<_>>();
    assert_eq!(frames.len(), 2);
    assert_eq!(&frames[0].payload_hex().unwrap()[..8], "cc8c0200");
    assert_eq!(&frames[1].payload_hex().unwrap()[..16], "cc8c020010010203");
    assert!(frames.iter().all(|step| step.caller_buffer_length() == 64));
}

#[test]
fn v5_rejects_empty_duplicate_unknown_and_wraparound_ids() {
    assert_eq!(
        api_v5::encode_static(&[]).unwrap_err(),
        EncodeError::EmptyAssignments
    );
    assert_eq!(
        api_v5::encode_static(&[
            LogicalColor::new(0, Rgb::new(1, 2, 3)),
            LogicalColor::new(0, Rgb::new(4, 5, 6)),
        ])
        .unwrap_err(),
        EncodeError::DuplicateLogicalId(0)
    );
    assert_eq!(
        api_v5::encode_static(&[LogicalColor::new(200, Rgb::new(1, 2, 3))]).unwrap_err(),
        EncodeError::UnknownLogicalId(200)
    );
    assert_eq!(
        api_v5::encode_static(&[LogicalColor::new(255, Rgb::new(1, 2, 3))]).unwrap_err(),
        EncodeError::EncodedIdOutOfRange(255)
    );
}

#[test]
fn v4_uses_exact_33_byte_direct_libusb_payloads_without_report_id() {
    let plan = api_v4::encode_static(&[
        LogicalColor::new(0, Rgb::new(255, 0, 0)),
        LogicalColor::new(2, Rgb::new(0, 255, 0)),
        LogicalColor::new(4, Rgb::new(0, 0, 255)),
    ])
    .unwrap();
    let steps = plan.steps();
    assert_eq!(plan.family(), ProtocolFamily::AlienFxApiV4);
    assert_step(
        &steps[0],
        TransferKind::UsbOutput,
        33,
        33,
        &[0x03, 0x21, 0x00, 0x04, 0xff, 0xff],
    );
    assert_step(
        &steps[1],
        TransferKind::UsbOutput,
        33,
        33,
        &[0x03, 0x21, 0x00, 0x01, 0xff, 0xff],
    );
    assert!(steps.iter().all(|step| step.caller_buffer_length() == 33));
    assert!(steps.iter().all(|step| step.on_wire_length() == 33));
    assert!(steps
        .iter()
        .all(|step| !step.payload_hex().unwrap().starts_with("00")));
    assert!(steps
        .iter()
        .any(|step| step.payload_hex().unwrap().starts_with("0327ff0000000100")));
    assert!(steps
        .iter()
        .any(|step| step.payload_hex().unwrap().starts_with("032700ff00000102")));
    assert!(steps
        .iter()
        .any(|step| step.payload_hex().unwrap().starts_with("03270000ff000104")));
    assert_step(
        steps.last().unwrap(),
        TransferKind::UsbOutput,
        33,
        33,
        &[0x03, 0x21, 0x00, 0x03, 0xff, 0xff],
    );
}

#[test]
fn v4_rejects_empty_duplicate_and_unknown_ids() {
    assert_eq!(
        api_v4::encode_static(&[]).unwrap_err(),
        EncodeError::EmptyAssignments
    );
    assert_eq!(
        api_v4::encode_static(&[
            LogicalColor::new(0, Rgb::new(1, 2, 3)),
            LogicalColor::new(0, Rgb::new(1, 2, 3)),
        ])
        .unwrap_err(),
        EncodeError::DuplicateLogicalId(0)
    );
    assert_eq!(
        api_v4::encode_static(&[LogicalColor::new(1, Rgb::new(1, 2, 3))]).unwrap_err(),
        EncodeError::UnknownLogicalId(1)
    );
}

#[test]
fn encoders_sort_assignments_deterministically() {
    let forward = api_v5::encode_static(&[
        LogicalColor::new(2, Rgb::new(3, 3, 3)),
        LogicalColor::new(0, Rgb::new(1, 1, 1)),
        LogicalColor::new(1, Rgb::new(2, 2, 2)),
    ])
    .unwrap();
    let reverse = api_v5::encode_static(&[
        LogicalColor::new(1, Rgb::new(2, 2, 2)),
        LogicalColor::new(0, Rgb::new(1, 1, 1)),
        LogicalColor::new(2, Rgb::new(3, 3, 3)),
    ])
    .unwrap();
    assert_eq!(forward, reverse);
}

#[test]
fn keyboard_mapping_supports_aliases_and_known_only_all_expansion() {
    assert_eq!(lookup_keyboard_target("escape").unwrap().logical_id, 0);
    assert_eq!(lookup_keyboard_target("esc").unwrap().logical_id, 0);
    assert_eq!(
        lookup_keyboard_target("left-windows").unwrap().logical_id,
        102
    );
    let all = expand_keyboard_targets(&["all"]).unwrap();
    assert_eq!(all.len(), keyboard_targets().len());
    assert!(all
        .windows(2)
        .all(|pair| pair[0].logical_id < pair[1].logical_id));
    assert_eq!(
        expand_keyboard_targets(&["all", "esc"]).unwrap_err(),
        TargetError::DuplicateTarget("escape")
    );
    assert!(expand_keyboard_targets(&["definitely-unknown"]).is_err());
}

#[test]
fn keyboard_known_validation_records_cover_only_individually_executed_keys() {
    let expected = [
        (
            "escape",
            0,
            "One individually executed static #ff0000 operation completed once after ready signature cc9317112100; the user visually confirmed only Escape red. No retry, readback, persistence, or automatic restore.",
        ),
        (
            "f1",
            1,
            "One individually executed static #ff0000 operation completed once after ready signature cc9317112100; the user visually confirmed only F1 red. No retry, readback, persistence, or automatic restore.",
        ),
        (
            "w",
            43,
            "One individually executed static #ff0000 operation completed once after ready signature cc9317112100; the user visually confirmed only W red. No retry, readback, persistence, or automatic restore.",
        ),
        (
            "space",
            106,
            "One individually executed static #ff0000 operation completed once after ready signature cc9317112100; the user visually confirmed the whole Space bar red. No retry, readback, persistence, or automatic restore.",
        ),
    ];
    let recorded = keyboard_targets()
        .iter()
        .filter_map(|target| target.known_validation.map(|record| (target, record)))
        .collect::<Vec<_>>();
    assert_eq!(recorded.len(), expected.len());
    for (name, logical_id, evidence) in expected {
        let (target, record) = recorded
            .iter()
            .find(|(target, _)| target.name == name)
            .copied()
            .unwrap();
        assert_eq!(target.logical_id, logical_id);
        assert_eq!(record.target, name);
        assert_eq!(record.logical_id, logical_id);
        assert_eq!(record.mode, alienrgb::model::ValidationMode::StaticColor);
        assert_eq!(record.color, "#ff0000");
        assert_eq!(record.device_profile, "Alienware m16 R2");
        assert_eq!(record.bios_version, "1.21.0");
        assert_eq!(record.controller, "0d62:d2b1");
        assert_eq!(record.evidence, evidence);
    }
    for name in ["f2", "q"] {
        assert!(lookup_keyboard_target(name)
            .unwrap()
            .known_validation
            .is_none());
    }
}

#[test]
fn keyboard_mapping_contains_the_complete_upstream_logical_id_set() {
    let ids = keyboard_targets()
        .iter()
        .map(|target| target.logical_id)
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        vec![
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
            24, 25, 26, 27, 28, 29, 30, 31, 32, 34, 40, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52,
            53, 55, 60, 62, 63, 64, 65, 66, 67, 68, 69, 70, 71, 72, 74, 80, 83, 84, 85, 86, 87, 88,
            89, 90, 91, 92, 94, 100, 101, 102, 104, 106, 109, 111, 112, 114, 133, 134, 135,
        ]
    );
}

#[test]
fn serialized_plan_uses_stable_lowercase_hex_fields() {
    let plan = api_v5::encode_static(&[LogicalColor::new(0, Rgb::new(10, 11, 12))]).unwrap();
    let json = serde_json::to_value(plan).unwrap();
    assert_eq!(json["family"], "alien_fx_api_v5");
    assert_eq!(json["assignments"][0]["color_hex"], "#0a0b0c");
    assert!(json["steps"][0]["payload_hex"]
        .as_str()
        .unwrap()
        .chars()
        .all(|character| character.is_ascii_digit() || ('a'..='f').contains(&character)));
}

#[test]
fn chassis_mapping_tracks_evidence_and_alias_duplicates() {
    let all = expand_chassis_targets(&["all"]).unwrap();
    assert_eq!(
        all.iter()
            .map(|target| target.logical_id)
            .collect::<Vec<_>>(),
        vec![0, 2, 4]
    );
    assert_eq!(
        all.iter()
            .map(|target| target.validation)
            .collect::<Vec<_>>(),
        vec![
            ValidationState::MappingDerivedUnvalidated,
            ValidationState::MappingDerivedUnvalidated,
            ValidationState::MappingDerivedUnvalidated,
        ]
    );
    assert_eq!(
        all.iter()
            .map(|target| (target.name, target.aliases, target.logical_id))
            .collect::<Vec<_>>(),
        vec![
            ("touchpad", &["haptic"][..], 0),
            ("back", &["chassis"][..], 2),
            ("power", &["power-button"][..], 4),
        ]
    );
    assert_eq!(
        all.iter()
            .filter_map(|target| target.known_validation)
            .map(|record| record.mode)
            .collect::<Vec<_>>(),
        vec![
            alienrgb::model::ValidationMode::StaticColor,
            alienrgb::model::ValidationMode::StaticColor,
            alienrgb::model::ValidationMode::PowerProfileEqualColor,
        ]
    );
    let record = all[0].known_validation.unwrap();
    assert_eq!(record.mode, alienrgb::model::ValidationMode::StaticColor);
    assert_eq!(record.color, "#ff0000");
    assert_eq!(record.device_profile, "Alienware m16 R2");
    assert_eq!(record.bios_version, "1.21.0");
    assert_eq!(record.controller, "187c:0551");
    assert_eq!(record.target, "touchpad");
    assert_eq!(record.logical_id, 0);
    assert!(record.evidence.contains("visually confirmed red"));
    let back_record = all[1].known_validation.unwrap();
    assert_eq!(
        back_record.mode,
        alienrgb::model::ValidationMode::StaticColor
    );
    assert_eq!(back_record.color, "#ff0000");
    assert_eq!(back_record.device_profile, "Alienware m16 R2");
    assert_eq!(back_record.bios_version, "1.21.0");
    assert_eq!(back_record.controller, "187c:0551");
    assert_eq!(back_record.target, "back");
    assert_eq!(back_record.logical_id, 2);
    assert!(back_record
        .evidence
        .contains("only rear lighting became red"));
    let power_record = all[2].known_validation.unwrap();
    assert_eq!(
        power_record.mode,
        alienrgb::model::ValidationMode::PowerProfileEqualColor
    );
    assert_eq!(power_record.color, "#ff0000");
    assert_eq!(power_record.device_profile, "Alienware m16 R2");
    assert_eq!(power_record.bios_version, "1.21.0");
    assert_eq!(power_record.controller, "187c:0551");
    assert_eq!(power_record.target, "power");
    assert_eq!(power_record.logical_id, 4);
    for evidence in [
        "34 ordered 33-byte writes",
        "AC online=0",
        "BAT0 Discharging",
        "capacity 22%",
        "battery-on/discharging visible behavior",
        "AC, sleep, charging, and battery-critical visual behavior remain unobserved",
        "Persistence unknown",
        "no readback or restore",
    ] {
        assert!(power_record.evidence.contains(evidence), "{evidence}");
    }
    assert!(all[1]
        .validation_detail
        .contains("generally mapping-derived beyond the attached record"));
    assert!(all[2]
        .validation_detail
        .contains("ordinary static/address semantics remain unvalidated"));

    let red = api_v4::encode_static(&[LogicalColor::new(0, Rgb::new(255, 0, 0))]).unwrap();
    let green = api_v4::encode_static(&[LogicalColor::new(0, Rgb::new(0, 255, 0))]).unwrap();
    assert_eq!(
        red.assignments()[0].validation(),
        ValidationState::ExactTouchpadStaticRedLiveValidated
    );
    assert_eq!(
        green.assignments()[0].validation(),
        ValidationState::MappingDerivedUnvalidated
    );
    assert_eq!(
        expand_chassis_targets(&["touchpad", "haptic"]).unwrap_err(),
        TargetError::DuplicateTarget("touchpad")
    );
}

#[test]
fn back_static_red_has_its_own_exact_validation_record_and_state() {
    let back = lookup_chassis_target("back").unwrap();
    let alias = lookup_chassis_target("chassis").unwrap();
    assert_eq!(back, alias);
    assert_eq!(back.logical_id, 2);

    let record = back.known_validation.unwrap();
    assert_eq!(record.mode, alienrgb::model::ValidationMode::StaticColor);
    assert_eq!(record.color, "#ff0000");
    assert_eq!(record.device_profile, "Alienware m16 R2");
    assert_eq!(record.bios_version, "1.21.0");
    assert_eq!(record.controller, "187c:0551");
    assert_eq!(record.target, "back");
    assert_eq!(record.logical_id, 2);
    assert_eq!(
        record.evidence,
        "One separately authorized guarded execution transferred remove/start/set_color/finish_play exactly once at 33 bytes each; the user confirmed only rear lighting became red. No retry, sudo, readback, restore, or persistence."
    );

    let red = api_v4::encode_static(&[LogicalColor::new(2, Rgb::new(255, 0, 0))]).unwrap();
    assert_eq!(
        red.assignments()[0].validation(),
        ValidationState::ExactBackStaticRedLiveValidated
    );
    assert_eq!(
        red.validation(),
        ValidationState::ExactBackStaticRedLiveValidated
    );
}

#[test]
fn v4_aggregates_exact_and_mixed_chassis_validation_without_mislabeling() {
    let touchpad = api_v4::encode_static(&[LogicalColor::new(0, Rgb::new(255, 0, 0))]).unwrap();
    let back = api_v4::encode_static(&[LogicalColor::new(2, Rgb::new(255, 0, 0))]).unwrap();
    let both = api_v4::encode_static(&[
        LogicalColor::new(0, Rgb::new(255, 0, 0)),
        LogicalColor::new(2, Rgb::new(255, 0, 0)),
    ])
    .unwrap();
    let exact_and_unvalidated = api_v4::encode_static(&[
        LogicalColor::new(2, Rgb::new(255, 0, 0)),
        LogicalColor::new(4, Rgb::new(255, 0, 0)),
    ])
    .unwrap();
    let green = api_v4::encode_static(&[LogicalColor::new(2, Rgb::new(0, 255, 0))]).unwrap();

    assert_eq!(
        touchpad.validation(),
        ValidationState::ExactTouchpadStaticRedLiveValidated
    );
    assert_eq!(
        back.validation(),
        ValidationState::ExactBackStaticRedLiveValidated
    );
    assert_eq!(
        both.validation(),
        ValidationState::MixedChassisTargetEvidence
    );
    assert_eq!(
        both.assumptions()[0].state(),
        ValidationState::ExactTouchpadStaticRedLiveValidated
    );
    assert_eq!(
        both.assumptions()[1].state(),
        ValidationState::ExactBackStaticRedLiveValidated
    );
    assert!(both.assumptions()[1]
        .detail()
        .contains("only rear lighting"));
    assert_eq!(
        both.assignments()[1].validation(),
        ValidationState::ExactBackStaticRedLiveValidated
    );
    assert_eq!(
        exact_and_unvalidated.validation(),
        ValidationState::MixedChassisTargetEvidence
    );
    assert_eq!(
        green.validation(),
        ValidationState::MappingDerivedUnvalidated
    );
}

#[test]
fn power_v4_emits_the_exhaustive_equal_color_set_power_action_plan() {
    let plan = power_v4::encode(Rgb::new(0x12, 0x34, 0x56));
    let p = [0x02, 0x03, 0xe8, 0x00, 0x64, 0x12, 0x34, 0x56];
    let c = [0x00, 0x03, 0xd0, 0x00, 0xfa, 0x12, 0x34, 0x56];
    let u = [0x01, 0x03, 0xdc, 0x00, 0x64, 0x12, 0x34, 0x56];
    let z = [0x02, 0x03, 0xe8, 0x00, 0x64, 0x00, 0x00, 0x00];
    let states = [
        (0x5b, "ac_sleep", vec![p, p, p, z]),
        (0x5c, "ac_on", vec![c, p, p]),
        (0x5d, "charging", vec![p, p, p, p]),
        (0x5e, "battery_sleep", vec![p, p, p, z]),
        (0x5f, "battery_on", vec![c, p, p]),
        (0x60, "battery_critical", vec![u, p, p]),
    ];
    let mut expected = Vec::new();
    let mut expected_names = Vec::new();
    for (cid, name, records) in states {
        expected.push(vec![0x03, 0x22, 0x00, 0x04, 0x00, cid]);
        expected_names.push(format!("state_{name}_remove"));
        expected.push(vec![0x03, 0x22, 0x00, 0x01, 0x00, cid]);
        expected_names.push(format!("state_{name}_start"));
        expected.push(vec![0x03, 0x23, 0x01, 0x00, 0x01, 0x04]);
        expected_names.push(format!("state_{name}_color_select"));
        for (packet_index, chunk) in records.chunks(3).enumerate() {
            let mut packet = vec![0x03, 0x24];
            for record in chunk {
                packet.extend_from_slice(record);
            }
            expected.push(packet);
            expected_names.push(format!("state_{name}_action_{}", packet_index + 1));
        }
        expected.push(vec![0x03, 0x22, 0x00, 0x02, 0x00, cid]);
        expected_names.push(format!("state_{name}_finish"));
    }
    expected.push(vec![0x03, 0x21, 0x00, 0x05, 0xff, 0xff]);
    expected_names.push("final_play".into());

    assert_eq!(plan.family(), ProtocolFamily::AlienFxApiV4);
    assert_eq!(
        plan.validation(),
        ValidationState::MappingDerivedUnvalidated
    );
    assert_eq!(plan.assignments()[0].logical_id(), 4);
    assert_eq!(plan.assignments()[0].color_hex(), "#123456");
    assert_eq!(plan.steps().len(), 34);
    assert_eq!(expected.len(), 34);
    for ((step, prefix), name) in plan.steps().iter().zip(expected).zip(expected_names) {
        assert_eq!(step.name(), name);
        assert_step(step, TransferKind::UsbOutput, 33, 33, &prefix);
        assert_eq!(step.payload_hex().unwrap().len(), 66);
        assert!(!step.payload_hex().unwrap().starts_with("00"));
    }
    assert_eq!(
        power_v4::STATES.map(|state| state.id),
        [0x5b, 0x5c, 0x5d, 0x5e, 0x5f, 0x60]
    );
    assert_eq!(
        power_v4::STATES.map(|state| state.packet_count),
        [6, 5, 6, 6, 5, 5]
    );
    assert_eq!(plan, power_v4::encode(Rgb::new(0x12, 0x34, 0x56)));

    let ordinary_red = api_v4::encode_static(&[LogicalColor::new(4, Rgb::new(255, 0, 0))]).unwrap();
    assert_eq!(
        ordinary_red.assignments()[0].validation(),
        ValidationState::MappingDerivedUnvalidated
    );
}

fn assert_step(
    step: &alienrgb::model::PacketStep,
    transfer: TransferKind,
    caller_length: usize,
    wire_length: usize,
    prefix: &[u8],
) {
    assert_eq!(step.transfer(), transfer);
    assert_eq!(step.caller_buffer_length(), caller_length);
    assert_eq!(step.on_wire_length(), wire_length);
    let expected = format!(
        "{}{}",
        hex(prefix),
        "00".repeat(caller_length - prefix.len())
    );
    assert_eq!(step.payload_hex(), Some(expected.as_str()));
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
