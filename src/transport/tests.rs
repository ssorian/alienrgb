use super::aw_elc::{
    acquire_aw_elc_handoff, validate_aw_opened, AwElcHandoff, AwInterfaceEvidence,
    AwOpenedEvidence, DriverState, EndpointEvidence,
};
use super::keyboard::{
    validate_keyboard_enumeration, validate_opened_keyboard, KeyboardEnumerationRecord,
    OpenedKeyboardEvidence,
};
use super::*;
use crate::model::{
    DescriptorStatus, EffectiveAccess, HidrawInfo, HidrawNodeState, PermissionEstimate,
    ValidationState,
};
use crate::profile::{DescriptorEvidence, KEYBOARD_DESCRIPTOR_SHA256};
use crate::protocol::{power_v4, Rgb};
use rusb::{Direction, TransferType};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Mutex;

static PROCESS_GUARD_TEST_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn canonical_mismatch_fails_before_fresh_discovery_or_acquisition() {
    let base = prepare_keyboard(keyboard_intent()).unwrap();
    let mut cases = Vec::new();
    let mut family = base.clone();
    family.canonical.family = ProtocolFamily::AlienFxApiV4;
    cases.push(family);
    let mut assignment = base.clone();
    assignment.canonical.assignments[0].target = "tampered".into();
    cases.push(assignment);
    let mut bytes = base.clone();
    bytes.canonical.steps[0]
        .payload_hex
        .as_mut()
        .unwrap()
        .replace_range(0..2, "00");
    cases.push(bytes);
    let mut metadata = base;
    metadata.canonical.assumptions[0].detail = "tampered";
    cases.push(metadata);

    for prepared in cases {
        let mut discovery = FakeDiscovery::supported();
        let mut factory = FakeKeyboardFactory::ready(0x8c);
        let error = execute_keyboard(prepared, &mut discovery, &mut factory).unwrap_err();
        assert_eq!(error.code, ExecutionErrorCode::CanonicalMismatch);
        assert_eq!(discovery.calls, 0);
        assert_eq!(factory.acquisitions, 0);
    }
}

#[test]
fn bounded_keyboard_live_intent_enforces_catalog_and_exact_color_frame_count() {
    let color = Rgb::new(0xff, 0, 0);
    for (count, expected_frames, expected_operations) in
        [(1, 1, 6), (15, 1, 6), (16, 2, 7), (85, 6, 11)]
    {
        let intent = keyboard_targets()
            .iter()
            .take(count)
            .map(|target| LogicalColor::new(target.logical_id, color))
            .collect::<Vec<_>>();
        let prepared = prepare_keyboard(intent).unwrap();
        assert_eq!(prepared.intent.len(), count);
        assert_eq!(
            prepared
                .canonical
                .steps()
                .iter()
                .filter(|step| step.name == "set_color")
                .count(),
            expected_frames
        );
        assert_eq!(prepared.canonical.steps().len(), expected_operations);
        if count == 85 {
            let mut discovery = FakeDiscovery::supported();
            let mut factory = FakeKeyboardFactory::ready(0x17);
            let state = factory.state.clone();
            let result = execute_keyboard(prepared, &mut discovery, &mut factory).unwrap();
            assert_eq!(result.steps.len(), 11);
            assert_eq!(result.steps[2].actual_length, 6);
            assert_eq!(state.borrow().operations.len(), 11);
            assert_eq!(state.borrow().writes.len(), 10);
        }
    }

    let mut over_catalog = keyboard_targets()
        .iter()
        .map(|target| LogicalColor::new(target.logical_id, color))
        .collect::<Vec<_>>();
    over_catalog.push(LogicalColor::new(0, color));
    let invalid = [
        Vec::new(),
        over_catalog,
        vec![
            LogicalColor::new(0, color),
            LogicalColor::new(1, Rgb::new(0, 0xff, 0)),
        ],
        vec![LogicalColor::new(0, color), LogicalColor::new(0, color)],
        vec![LogicalColor::new(2, color), LogicalColor::new(1, color)],
        vec![LogicalColor::new(200, color)],
    ];
    for intent in invalid {
        assert_eq!(
            prepare_keyboard(intent).unwrap_err().code,
            ExecutionErrorCode::InvalidTarget
        );
    }
}

#[test]
fn unknown_intent_cannot_produce_a_prepared_execution() {
    let error = prepare_keyboard(vec![LogicalColor::new(200, Rgb::new(1, 2, 3))]).unwrap_err();
    assert_eq!(error.code, ExecutionErrorCode::InvalidTarget);
    let error = prepare_aw_elc(vec![LogicalColor::new(1, Rgb::new(1, 2, 3))]).unwrap_err();
    assert_eq!(error.code, ExecutionErrorCode::InvalidTarget);
}

#[test]
fn fresh_keyboard_discovery_requires_uniqueness_interface_and_hash() {
    let dmi = supported_dmi();
    let keyboard = keyboard_device();
    let selection = select_keyboard(&dmi, std::slice::from_ref(&keyboard)).unwrap();
    assert_eq!(selection.path, std::path::PathBuf::from("/dev/hidraw-test"));

    assert_eq!(
        select_keyboard(&dmi, &[keyboard.clone(), keyboard.clone()])
            .unwrap_err()
            .code,
        ExecutionErrorCode::AmbiguousTarget
    );
    let mut wrong_hash = keyboard.clone();
    wrong_hash.descriptor.as_mut().unwrap().evidence = DescriptorEvidence::CompatibleSignature;
    assert_eq!(
        select_keyboard(&dmi, &[wrong_hash]).unwrap_err().code,
        ExecutionErrorCode::InvalidTarget
    );
    let mut ambiguous_path = keyboard;
    ambiguous_path.hidraw.push(ambiguous_path.hidraw[0].clone());
    assert_eq!(
        select_keyboard(&dmi, &[ambiguous_path]).unwrap_err().code,
        ExecutionErrorCode::AmbiguousTarget
    );
}

#[test]
fn keyboard_enumeration_accepts_real_multi_collection_shape_and_ignores_other_paths() {
    let records = vec![
        keyboard_enumeration_record("/dev/hidraw-test", 0x0001, 0x0001),
        keyboard_enumeration_record("/dev/hidraw-test", 0xff89, 0x00cc),
        keyboard_enumeration_record("/dev/hidraw-test", 0x000c, 0x0001),
        keyboard_enumeration_record("/dev/hidraw-other", 0xff89, 0x00cc),
    ];
    assert_eq!(
        validate_keyboard_enumeration(b"/dev/hidraw-test", &records).unwrap(),
        1
    );
}

#[test]
fn keyboard_enumeration_rejects_duplicate_rgb_collection() {
    let records = vec![
        keyboard_enumeration_record("/dev/hidraw-test", 0xff89, 0x00cc),
        keyboard_enumeration_record("/dev/hidraw-test", 0xff89, 0x00cc),
    ];
    assert!(validate_keyboard_enumeration(b"/dev/hidraw-test", &records).is_err());
}

#[test]
fn keyboard_enumeration_rejects_absent_rgb_collection() {
    let records = vec![
        keyboard_enumeration_record("/dev/hidraw-test", 0x0001, 0x0001),
        keyboard_enumeration_record("/dev/hidraw-test", 0x000c, 0x0001),
    ];
    assert!(validate_keyboard_enumeration(b"/dev/hidraw-test", &records).is_err());
}

#[test]
fn keyboard_enumeration_rejects_conflicting_same_path_metadata() {
    let valid = keyboard_enumeration_record("/dev/hidraw-test", 0xff89, 0x00cc);
    let mut conflicts = Vec::new();
    let mut bus = valid.clone();
    bus.is_usb = false;
    conflicts.push(bus);
    let mut vid = valid.clone();
    vid.vendor_id = 0xffff;
    conflicts.push(vid);
    let mut pid = valid.clone();
    pid.product_id = 0xffff;
    conflicts.push(pid);
    let mut interface = valid.clone();
    interface.interface_number = 1;
    conflicts.push(interface);
    let mut manufacturer = valid.clone();
    manufacturer.manufacturer = Some("conflict".into());
    conflicts.push(manufacturer);
    let mut product = valid.clone();
    product.product = Some("conflict".into());
    conflicts.push(product);

    for conflict in conflicts {
        assert!(
            validate_keyboard_enumeration(b"/dev/hidraw-test", &[valid.clone(), conflict]).is_err()
        );
    }
}

#[test]
fn keyboard_enumeration_rejects_missing_exact_path() {
    let records = vec![keyboard_enumeration_record(
        "/dev/hidraw-other",
        0xff89,
        0x00cc,
    )];
    assert!(validate_keyboard_enumeration(b"/dev/hidraw-test", &records).is_err());
}

#[test]
fn opened_keyboard_identity_and_descriptor_hash_fail_closed() {
    let selection = KeyboardSelection {
        path: "/dev/hidraw-test".into(),
    };
    let valid = OpenedKeyboardEvidence {
        vendor_id: 0x0d62,
        product_id: 0xd2b1,
        interface_number: 0,
        path: "/dev/hidraw-test".into(),
        report_descriptor_sha256: KEYBOARD_DESCRIPTOR_SHA256.into(),
    };
    validate_opened_keyboard(&selection, &valid).unwrap();

    let mut wrong_hash = valid.clone();
    wrong_hash.report_descriptor_sha256 = "00".repeat(32);
    assert_eq!(
        validate_opened_keyboard(&selection, &wrong_hash)
            .unwrap_err()
            .code,
        ExecutionErrorCode::IdentityDrift
    );
    let mut wrong_interface = valid.clone();
    wrong_interface.interface_number = 1;
    assert_eq!(
        validate_opened_keyboard(&selection, &wrong_interface)
            .unwrap_err()
            .code,
        ExecutionErrorCode::InterfaceDrift
    );
    let mut wrong_path = valid;
    wrong_path.path = "/dev/other".into();
    assert_eq!(
        validate_opened_keyboard(&selection, &wrong_path)
            .unwrap_err()
            .code,
        ExecutionErrorCode::IdentityDrift
    );
}

#[test]
fn keyboard_status_capture_is_exactly_query_then_read_and_accepts_six_bytes() {
    let mut discovery = FakeDiscovery::supported();
    let mut factory = FakeKeyboardFactory::ready(0x8c);
    factory.state.borrow_mut().read_response = Some(vec![0xcc, 0x01, 0x80, 0x02, 0xaa, 0xff]);
    factory.state.borrow_mut().read_length = Some(6);
    let state = factory.state.clone();

    let capture = execute_keyboard_status(&mut discovery, &mut factory).unwrap();
    let state = state.borrow();
    assert_eq!(discovery.calls, 1);
    assert_eq!(factory.acquisitions, 1);
    assert_eq!(state.operations, ["write", "read"]);
    assert_eq!(state.feature_lengths, [64, 64]);
    assert_eq!(state.writes.len(), 1);
    assert_eq!(&state.writes[0][..2], &[0xcc, 0x93]);
    assert!(state.writes[0][2..].iter().all(|byte| *byte == 0));
    assert_eq!(state.read_initial_report_ids, [0xcc]);
    assert_eq!(capture.query_write_length, 64);
    assert_eq!(capture.response, [0xcc, 0x01, 0x80, 0x02, 0xaa, 0xff]);
}

#[test]
fn keyboard_status_capture_keeps_accepting_uninterpreted_lengths_one_through_sixty_four() {
    for length in [1, 7, 64] {
        let mut discovery = FakeDiscovery::supported();
        let mut factory = FakeKeyboardFactory::ready(0x80);
        let mut response = vec![0x55; length];
        response[0] = 0xcc;
        factory.state.borrow_mut().read_response = Some(response.clone());
        factory.state.borrow_mut().read_length = Some(length);
        let capture = execute_keyboard_status(&mut discovery, &mut factory).unwrap();
        assert_eq!(capture.response, response);
    }
}

#[test]
fn keyboard_status_capture_rejects_invalid_lengths_id_and_errors_without_retry() {
    for (length, report_id, fail_read) in [
        (0, 0xcc, false),
        (65, 0xcc, false),
        (6, 0x00, false),
        (6, 0xcc, true),
    ] {
        let mut discovery = FakeDiscovery::supported();
        let mut factory = FakeKeyboardFactory::response(report_id, 0x8c);
        factory.state.borrow_mut().read_length = Some(length);
        factory.state.borrow_mut().fail_read = fail_read;
        let state = factory.state.clone();
        assert!(execute_keyboard_status(&mut discovery, &mut factory).is_err());
        assert_eq!(factory.acquisitions, 1);
        assert_eq!(state.borrow().operations, ["write", "read"]);
        assert_eq!(state.borrow().write_count, 1);
    }
}

#[test]
fn keyboard_process_guard_rejects_both_paths_and_releases_via_raii() {
    let _test_lock = PROCESS_GUARD_TEST_LOCK.lock().unwrap();
    {
        let _held = KeyboardExecutionGuard::try_acquire().unwrap();

        let mut status_discovery = FakeDiscovery::supported();
        let mut status_factory = FakeKeyboardFactory::ready(0x8c);
        status_factory.guarded = true;
        let status_error =
            execute_keyboard_status(&mut status_discovery, &mut status_factory).unwrap_err();
        assert_eq!(status_error.code, ExecutionErrorCode::Busy);
        assert_eq!(status_discovery.calls, 0);
        assert_eq!(status_factory.acquisitions, 0);

        let prepared = prepare_keyboard(keyboard_intent()).unwrap();
        let mut set_discovery = FakeDiscovery::supported();
        let mut set_factory = FakeKeyboardFactory::ready(0x8c);
        set_factory.guarded = true;
        let set_error =
            execute_keyboard(prepared, &mut set_discovery, &mut set_factory).unwrap_err();
        assert_eq!(set_error.code, ExecutionErrorCode::Busy);
        assert_eq!(set_discovery.calls, 0);
        assert_eq!(set_factory.acquisitions, 0);
    }

    let mut discovery = FakeDiscovery::supported();
    let mut factory = FakeKeyboardFactory::ready(0x8c);
    factory.guarded = true;
    execute_keyboard_status(&mut discovery, &mut factory).unwrap();
    assert_eq!(discovery.calls, 1);
    assert_eq!(factory.acquisitions, 1);

    let _released_after_execution = KeyboardExecutionGuard::try_acquire().unwrap();
}

#[test]
fn keyboard_permission_failure_stops_before_any_feature_operation() {
    let mut discovery = FakeDiscovery::supported();
    let mut factory = FakeKeyboardFactory::permission_denied();
    let state = factory.state.clone();
    let error = execute_keyboard(
        prepare_keyboard(keyboard_intent()).unwrap(),
        &mut discovery,
        &mut factory,
    )
    .unwrap_err();
    assert_eq!(error.code, ExecutionErrorCode::AcquisitionFailed);
    assert!(error.message.contains("keyboard_permission_denied"));
    assert!(state.borrow().operations.is_empty());
}

#[test]
fn keyboard_accepts_only_observed_m16_r2_status_ready_signature() {
    let mut discovery = FakeDiscovery::supported();
    let mut factory = FakeKeyboardFactory::ready(0x17);
    factory.state.borrow_mut().read_response =
        Some(KEYBOARD_OBSERVED_STATUS_READY_SIGNATURE.to_vec());
    factory.state.borrow_mut().read_length = Some(KEYBOARD_OBSERVED_STATUS_READY_SIGNATURE.len());
    let result = execute_keyboard(
        prepare_keyboard(keyboard_intent()).unwrap(),
        &mut discovery,
        &mut factory,
    )
    .unwrap();
    assert_eq!(result.status_byte, Some(0x17));
    assert!(result.completed);
    assert_eq!(
        factory.state.borrow().operations,
        ["write", "write", "read", "write", "write", "write"]
    );
    assert_eq!(factory.state.borrow().write_count, 5);
}

#[test]
fn keyboard_observed_ready_signature_is_scoped_to_confirmed_bios() {
    for bios_version in [None, Some("1.22.0".into())] {
        let mut discovery = FakeDiscovery::supported();
        discovery.inventory.0.bios_version = bios_version;
        let mut factory = FakeKeyboardFactory::ready(0x17);
        let error = execute_keyboard(
            prepare_keyboard(keyboard_intent()).unwrap(),
            &mut discovery,
            &mut factory,
        )
        .unwrap_err();
        assert_eq!(error.code, ExecutionErrorCode::InvalidTarget);
        assert_eq!(
            error.message,
            "keyboard live status readiness is pinned to Alienware m16 R2 BIOS 1.21.0"
        );
        assert_eq!(factory.acquisitions, 0);
    }

    let mut discovery = FakeDiscovery::supported();
    let mut factory = FakeKeyboardFactory::ready(0x17);
    execute_keyboard(
        prepare_keyboard(keyboard_intent()).unwrap(),
        &mut discovery,
        &mut factory,
    )
    .unwrap();
    assert_eq!(factory.acquisitions, 1);
}

#[test]
fn keyboard_rejects_every_one_byte_signature_mutation_before_color() {
    for index in 0..KEYBOARD_OBSERVED_STATUS_READY_SIGNATURE.len() {
        let mut response = KEYBOARD_OBSERVED_STATUS_READY_SIGNATURE;
        response[index] = if index == 2 {
            0x80
        } else {
            response[index] ^ 0x01
        };
        let mut discovery = FakeDiscovery::supported();
        let mut factory = FakeKeyboardFactory::ready(response[2]);
        factory.state.borrow_mut().read_response = Some(response.to_vec());
        factory.state.borrow_mut().read_length = Some(response.len());
        let state = factory.state.clone();
        let error = execute_keyboard(
            prepare_keyboard(keyboard_intent()).unwrap(),
            &mut discovery,
            &mut factory,
        )
        .unwrap_err();
        assert_eq!(
            error.code,
            match index {
                0 | 1 => ExecutionErrorCode::MalformedStatus,
                2 => ExecutionErrorCode::WaitUpdate,
                _ => ExecutionErrorCode::UnknownStatus,
            }
        );
        assert_eq!(state.borrow().operations, ["write", "write", "read"]);
        assert_eq!(state.borrow().write_count, 2);
    }
}

#[test]
fn keyboard_waitupdate_rejects_arbitrary_trailing_bytes_before_color() {
    let response = [0xcc, 0x93, 0x80, 0xde, 0xad, 0xbe];
    let mut discovery = FakeDiscovery::supported();
    let mut factory = FakeKeyboardFactory::ready(0x80);
    factory.state.borrow_mut().read_response = Some(response.to_vec());
    let state = factory.state.clone();
    let error = execute_keyboard(
        prepare_keyboard(keyboard_intent()).unwrap(),
        &mut discovery,
        &mut factory,
    )
    .unwrap_err();
    assert_eq!(error.code, ExecutionErrorCode::WaitUpdate);
    assert_eq!(state.borrow().operations, ["write", "write", "read"]);
    assert_eq!(state.borrow().write_count, 2);
}

#[test]
fn upstream_ready_constants_are_not_device_specific_readiness() {
    for status in [0x8c, 0xcc] {
        let mut response = KEYBOARD_OBSERVED_STATUS_READY_SIGNATURE;
        response[2] = status;
        let mut discovery = FakeDiscovery::supported();
        let mut factory = FakeKeyboardFactory::ready(status);
        factory.state.borrow_mut().read_response = Some(response.to_vec());
        let state = factory.state.clone();
        let error = execute_keyboard(
            prepare_keyboard(keyboard_intent()).unwrap(),
            &mut discovery,
            &mut factory,
        )
        .unwrap_err();
        assert_eq!(error.code, ExecutionErrorCode::UnknownStatus);
        assert_eq!(state.borrow().write_count, 2);
    }
}

#[test]
fn keyboard_unknown_status_signature_reports_full_lowercase_hex() {
    let response = [0xcc, 0x93, 0x17, 0x11, 0x21, 0xab];
    let mut discovery = FakeDiscovery::supported();
    let mut factory = FakeKeyboardFactory::ready(response[2]);
    factory.state.borrow_mut().read_response = Some(response.to_vec());
    factory.state.borrow_mut().read_length = Some(response.len());
    let error = execute_keyboard(
        prepare_keyboard(keyboard_intent()).unwrap(),
        &mut discovery,
        &mut factory,
    )
    .unwrap_err();
    assert_eq!(error.code, ExecutionErrorCode::UnknownStatus);
    assert!(error.message.contains("cc93171121ab"));
}

#[test]
fn keyboard_live_set_rejects_non_six_byte_status_before_color() {
    for length in [5, 7, 64] {
        let mut discovery = FakeDiscovery::supported();
        let mut factory = FakeKeyboardFactory::ready(0x17);
        factory.state.borrow_mut().read_response =
            Some(KEYBOARD_OBSERVED_STATUS_READY_SIGNATURE.to_vec());
        factory.state.borrow_mut().read_length = Some(length);
        let state = factory.state.clone();
        let error = execute_keyboard(
            prepare_keyboard(keyboard_intent()).unwrap(),
            &mut discovery,
            &mut factory,
        )
        .unwrap_err();
        assert_eq!(error.code, ExecutionErrorCode::ShortTransfer);
        assert_eq!(state.borrow().operations, ["write", "write", "read"]);
        assert_eq!(state.borrow().write_count, 2);
    }
}

#[test]
fn keyboard_read_error_and_short_status_stop_before_color_without_replay() {
    for short in [false, true] {
        let mut discovery = FakeDiscovery::supported();
        let mut factory = FakeKeyboardFactory::ready(0x8c);
        if short {
            factory.state.borrow_mut().read_length = Some(5);
        } else {
            factory.state.borrow_mut().fail_read = true;
        }
        let state = factory.state.clone();
        let error = execute_keyboard(
            prepare_keyboard(keyboard_intent()).unwrap(),
            &mut discovery,
            &mut factory,
        )
        .unwrap_err();
        assert_eq!(
            error.code,
            if short {
                ExecutionErrorCode::ShortTransfer
            } else {
                ExecutionErrorCode::BackendFailed
            }
        );
        assert_eq!(state.borrow().operations, vec!["write", "write", "read"]);
    }
}

#[test]
fn keyboard_multiframe_failure_stops_after_second_color_frame_without_retry() {
    let color = Rgb::new(0xff, 0, 0);
    let intent = keyboard_targets()
        .iter()
        .take(16)
        .map(|target| LogicalColor::new(target.logical_id, color))
        .collect::<Vec<_>>();
    let mut discovery = FakeDiscovery::supported();
    let mut factory = FakeKeyboardFactory::ready(0x17);
    factory.state.borrow_mut().fail_write_at = Some(3);
    let state = factory.state.clone();
    let error = execute_keyboard(
        prepare_keyboard(intent).unwrap(),
        &mut discovery,
        &mut factory,
    )
    .unwrap_err();
    assert_eq!(error.code, ExecutionErrorCode::BackendFailed);
    assert_eq!(
        state.borrow().operations,
        ["write", "write", "read", "write", "write"]
    );
    assert_eq!(state.borrow().write_count, 4);
}

#[test]
fn keyboard_short_transfer_and_backend_error_never_replay() {
    let mut discovery = FakeDiscovery::supported();
    let mut factory = FakeKeyboardFactory::ready(0x8c);
    factory.state.borrow_mut().short_write_at = Some(2);
    let state = factory.state.clone();
    assert_eq!(
        execute_keyboard(
            prepare_keyboard(keyboard_intent()).unwrap(),
            &mut discovery,
            &mut factory,
        )
        .unwrap_err()
        .code,
        ExecutionErrorCode::ShortTransfer
    );
    assert_eq!(state.borrow().write_count, 3);

    let mut discovery = FakeDiscovery::supported();
    let mut factory = FakeKeyboardFactory::ready(0x8c);
    factory.state.borrow_mut().fail_write_at = Some(2);
    let state = factory.state.clone();
    assert_eq!(
        execute_keyboard(
            prepare_keyboard(keyboard_intent()).unwrap(),
            &mut discovery,
            &mut factory,
        )
        .unwrap_err()
        .code,
        ExecutionErrorCode::BackendFailed
    );
    assert_eq!(state.borrow().write_count, 3);
}

#[test]
fn aw_selection_binds_unique_bus_port_and_serial() {
    let dmi = supported_dmi();
    let aw = aw_device();
    let selection = select_aw_elc(&dmi, std::slice::from_ref(&aw)).unwrap();
    assert_eq!(selection.bus_number, 3);
    assert_eq!(selection.port_path, vec![2, 4]);
    assert_eq!(selection.serial.as_deref(), Some("bound-serial"));
    assert_eq!(
        select_aw_elc(&dmi, &[aw.clone(), aw]).unwrap_err().code,
        ExecutionErrorCode::AmbiguousTarget
    );
}

#[test]
fn aw_live_execution_requires_confirmed_bios_before_acquisition() {
    for bios_version in [None, Some("1.22.0".into())] {
        let prepared = prepare_aw_elc(vec![LogicalColor::new(0, Rgb::new(1, 2, 3))]).unwrap();
        let mut discovery = FakeDiscovery::supported();
        discovery.inventory.0.bios_version = bios_version;
        let mut factory = FakeAwFactory::default();
        let error = execute_aw_elc(prepared, &mut discovery, &mut factory).unwrap_err();
        assert_eq!(error.code, ExecutionErrorCode::InvalidTarget);
        assert_eq!(
            error.message,
            "AW-ELC live execution is pinned to Alienware m16 R2 BIOS 1.21.0"
        );
        assert_eq!(discovery.calls, 1);
        assert_eq!(factory.acquisitions, 0);
        assert!(factory.state.borrow().writes.is_empty());
    }

    let prepared = prepare_aw_elc(vec![LogicalColor::new(0, Rgb::new(1, 2, 3))]).unwrap();
    let mut discovery = FakeDiscovery::supported();
    let mut factory = FakeAwFactory::default();
    execute_aw_elc(prepared, &mut discovery, &mut factory).unwrap();
    assert_eq!(factory.acquisitions, 1);
}

#[test]
fn aw_opened_identity_interface_endpoints_and_driver_state_fail_closed() {
    let selection = AwElcSelection {
        bus_number: 3,
        port_path: vec![2, 4],
        serial: Some("bound-serial".into()),
    };
    let valid = valid_aw_evidence();
    validate_aw_opened(&selection, &valid).unwrap();

    let mut bus_drift = valid.clone();
    bus_drift.bus_number = 4;
    assert_code(&selection, &bus_drift, ExecutionErrorCode::IdentityDrift);
    let mut interface_drift = valid.clone();
    interface_drift.interface.protocol = 1;
    assert_code(
        &selection,
        &interface_drift,
        ExecutionErrorCode::InterfaceDrift,
    );
    let mut endpoint_drift = valid.clone();
    endpoint_drift.interface.endpoints[0].max_packet_size = 34;
    assert_code(
        &selection,
        &endpoint_drift,
        ExecutionErrorCode::EndpointDrift,
    );
    let mut active = valid.clone();
    active.driver_after_claim = DriverState::Active;
    assert_code(&selection, &active, ExecutionErrorCode::DriverActive);
    let mut unknown = valid;
    unknown.driver_after_claim = DriverState::Unknown;
    assert_code(&selection, &unknown, ExecutionErrorCode::DriverStateUnknown);
}

#[test]
fn aw_dispatch_is_ordered_and_stops_without_replay() {
    let prepared = prepare_aw_elc(vec![LogicalColor::new(0, Rgb::new(1, 2, 3))]).unwrap();
    let mut discovery = FakeDiscovery::supported();
    let mut factory = FakeAwFactory::default();
    let state = factory.state.clone();
    let result = execute_aw_elc(prepared, &mut discovery, &mut factory).unwrap();
    assert_eq!(state.borrow().writes.len(), 4);
    assert_eq!(result.steps.len(), 4);

    let mut discovery = FakeDiscovery::supported();
    let mut factory = FakeAwFactory::default();
    factory.state.borrow_mut().fail_at = Some(2);
    let state = factory.state.clone();
    assert_eq!(
        execute_aw_elc(
            prepare_aw_elc(vec![LogicalColor::new(0, Rgb::new(1, 2, 3))]).unwrap(),
            &mut discovery,
            &mut factory,
        )
        .unwrap_err()
        .code,
        ExecutionErrorCode::BackendFailed
    );
    assert_eq!(state.borrow().writes.len(), 3);
}

#[test]
fn power_profile_dispatches_exactly_34_canonical_writes_once() {
    let color = Rgb::new(0x12, 0x34, 0x56);
    let mut discovery = FakeDiscovery::supported();
    let mut factory = FakeAwFactory::default();
    let state = factory.state.clone();
    let result =
        execute_power_profile(prepare_power_profile(color), &mut discovery, &mut factory).unwrap();
    let expected = power_v4::encode_equal_color(color);
    let state = state.borrow();
    assert_eq!(discovery.calls, 1);
    assert_eq!(factory.acquisitions, 1);
    assert_eq!(result.steps.len(), 34);
    assert!(result.completed);
    assert_eq!(state.writes.len(), 34);
    for (actual, step) in state.writes.iter().zip(expected.steps()) {
        assert_eq!(actual.len(), 33);
        assert_eq!(*actual, decode_payload(step, 33).unwrap());
    }
}

#[test]
fn power_profile_canonical_mismatch_stops_before_discovery() {
    let mut cases = Vec::new();
    let base = prepare_power_profile(Rgb::new(1, 2, 3));
    let mut family = base.clone();
    family.canonical.family = ProtocolFamily::AlienFxApiV5;
    cases.push(family);
    let mut metadata = base.clone();
    metadata.canonical.assignments[0].target = "tampered".into();
    cases.push(metadata);
    let mut payload = base;
    payload.canonical.steps[10]
        .payload_hex
        .as_mut()
        .unwrap()
        .replace_range(0..2, "00");
    cases.push(payload);

    for prepared in cases {
        let mut discovery = FakeDiscovery::supported();
        let mut factory = FakeAwFactory::default();
        let error = execute_power_profile(prepared, &mut discovery, &mut factory).unwrap_err();
        assert_eq!(error.code, ExecutionErrorCode::CanonicalMismatch);
        assert_eq!(discovery.calls, 0);
        assert_eq!(factory.acquisitions, 0);
        assert!(factory.state.borrow().writes.is_empty());
    }
}

#[test]
fn power_profile_validator_rejects_family_count_name_type_length_and_payload() {
    let color = Rgb::new(1, 2, 3);
    let base = power_v4::encode_equal_color(color);
    let mut cases = Vec::new();
    let mut family = base.clone();
    family.family = ProtocolFamily::AlienFxApiV5;
    cases.push((family, ExecutionErrorCode::InvalidFamily));
    let mut count = base.clone();
    count.steps.pop();
    cases.push((count, ExecutionErrorCode::InvalidStep));
    let mut name = base.clone();
    name.steps[3].name = "state_ac_sleep_action_2";
    cases.push((name, ExecutionErrorCode::InvalidStep));
    let mut transfer = base.clone();
    transfer.steps[3].transfer = TransferKind::HidFeatureWrite;
    cases.push((transfer, ExecutionErrorCode::InvalidTransfer));
    let mut caller_length = base.clone();
    caller_length.steps[3].caller_buffer_length = 34;
    cases.push((caller_length, ExecutionErrorCode::InvalidLength));
    let mut wire_length = base.clone();
    wire_length.steps[3].on_wire_length = 34;
    cases.push((wire_length, ExecutionErrorCode::InvalidLength));
    let mut payload = base;
    payload.steps[3]
        .payload_hex
        .as_mut()
        .unwrap()
        .replace_range(64..66, "01");
    cases.push((payload, ExecutionErrorCode::InvalidPayload));

    for (plan, expected) in cases {
        assert_eq!(
            validate_power_profile_plan(&plan, color).unwrap_err().code,
            expected
        );
    }
}

#[test]
fn power_profile_failures_abort_without_retry_or_later_packets() {
    for failure_index in [0, 16, 32, 33] {
        for short in [false, true] {
            let mut discovery = FakeDiscovery::supported();
            let mut factory = FakeAwFactory::default();
            if short {
                factory.state.borrow_mut().short_at = Some(failure_index);
            } else {
                factory.state.borrow_mut().fail_at = Some(failure_index);
            }
            let state = factory.state.clone();
            let error = execute_power_profile(
                prepare_power_profile(Rgb::new(1, 2, 3)),
                &mut discovery,
                &mut factory,
            )
            .unwrap_err();
            assert_eq!(
                error.code,
                if short {
                    ExecutionErrorCode::ShortTransfer
                } else {
                    ExecutionErrorCode::BackendFailed
                }
            );
            assert_eq!(state.borrow().writes.len(), failure_index + 1);
            assert_eq!(
                state
                    .borrow()
                    .writes
                    .iter()
                    .filter(|bytes| bytes.starts_with(&[0x03, 0x21, 0x00, 0x05]))
                    .count(),
                usize::from(failure_index == 33)
            );
        }
    }
}

#[test]
fn power_profile_discovery_identity_bios_and_acquisition_fail_before_writes() {
    for defect in ["discovery", "dmi", "bios", "device", "acquisition"] {
        let mut discovery = FakeDiscovery::supported();
        let mut factory = FakeAwFactory::default();
        match defect {
            "discovery" => discovery.fail = true,
            "dmi" => discovery.inventory.0.supported = false,
            "bios" => discovery.inventory.0.bios_version = Some("1.22.0".into()),
            "device" => discovery
                .inventory
                .1
                .retain(|device| device.kind != DeviceKind::Chassis),
            "acquisition" => factory.acquisition_error = Some("fake_acquisition_failed"),
            _ => unreachable!(),
        }
        let state = factory.state.clone();
        assert!(execute_power_profile(
            prepare_power_profile(Rgb::new(1, 2, 3)),
            &mut discovery,
            &mut factory,
        )
        .is_err());
        assert!(state.borrow().writes.is_empty(), "{defect}");
    }
}

#[test]
fn shared_aw_guard_blocks_static_and_power_before_discovery_and_releases() {
    let _test_lock = PROCESS_GUARD_TEST_LOCK.lock().unwrap();
    assert!(RusbAwElcFactory.requires_process_guard());
    {
        let _held = AwElcExecutionGuard::try_acquire().unwrap();
        let mut static_discovery = FakeDiscovery::supported();
        let mut static_factory = FakeAwFactory {
            guarded: true,
            ..FakeAwFactory::default()
        };
        let static_error = execute_aw_elc(
            prepare_aw_elc(vec![LogicalColor::new(0, Rgb::new(1, 2, 3))]).unwrap(),
            &mut static_discovery,
            &mut static_factory,
        )
        .unwrap_err();
        assert_eq!(static_error.code, ExecutionErrorCode::Busy);
        assert_eq!(static_discovery.calls, 0);
        assert_eq!(static_factory.acquisitions, 0);

        let mut power_discovery = FakeDiscovery::supported();
        let mut power_factory = FakeAwFactory {
            guarded: true,
            ..FakeAwFactory::default()
        };
        let power_error = execute_power_profile(
            prepare_power_profile(Rgb::new(1, 2, 3)),
            &mut power_discovery,
            &mut power_factory,
        )
        .unwrap_err();
        assert_eq!(power_error.code, ExecutionErrorCode::Busy);
        assert_eq!(power_discovery.calls, 0);
        assert_eq!(power_factory.acquisitions, 0);
    }

    let mut discovery = FakeDiscovery::supported();
    let mut factory = FakeAwFactory {
        guarded: true,
        ..FakeAwFactory::default()
    };
    execute_power_profile(
        prepare_power_profile(Rgb::new(1, 2, 3)),
        &mut discovery,
        &mut factory,
    )
    .unwrap();
    let _released = AwElcExecutionGuard::try_acquire().unwrap();
}

fn assert_code(
    selection: &AwElcSelection,
    evidence: &AwOpenedEvidence,
    expected: ExecutionErrorCode,
) {
    assert_eq!(
        validate_aw_opened(selection, evidence).unwrap_err().code,
        expected
    );
}

fn valid_aw_evidence() -> AwOpenedEvidence {
    AwOpenedEvidence {
        bus_number: 3,
        port_path: vec![2, 4],
        vendor_id: 0x187c,
        product_id: 0x0551,
        serial: Some("bound-serial".into()),
        interface: AwInterfaceEvidence {
            number: 0,
            class: 3,
            subclass: 0,
            protocol: 0,
            endpoints: vec![
                EndpointEvidence {
                    address: 0x01,
                    direction: Direction::Out,
                    transfer_type: TransferType::Interrupt,
                    max_packet_size: 33,
                },
                EndpointEvidence {
                    address: 0x81,
                    direction: Direction::In,
                    transfer_type: TransferType::Interrupt,
                    max_packet_size: 33,
                },
            ],
        },
        driver_before_claim: DriverState::Inactive,
        driver_after_claim: DriverState::Inactive,
    }
}

fn keyboard_intent() -> Vec<LogicalColor> {
    vec![LogicalColor::new(0, Rgb::new(1, 2, 3))]
}

struct FakeDiscovery {
    calls: usize,
    inventory: (DmiIdentity, Vec<DeviceSummary>),
    fail: bool,
}

impl FakeDiscovery {
    fn supported() -> Self {
        Self {
            calls: 0,
            inventory: (supported_dmi(), vec![keyboard_device(), aw_device()]),
            fail: false,
        }
    }
}

impl FreshDiscovery for FakeDiscovery {
    fn discover(&mut self) -> Result<(DmiIdentity, Vec<DeviceSummary>), BackendError> {
        self.calls += 1;
        if self.fail {
            Err(BackendError::new("fake_discovery_failed"))
        } else {
            Ok(self.inventory.clone())
        }
    }
}

#[derive(Default)]
struct KeyboardState {
    operations: Vec<&'static str>,
    report_id: u8,
    status: u8,
    write_count: usize,
    short_write_at: Option<usize>,
    fail_write_at: Option<usize>,
    feature_lengths: Vec<usize>,
    writes: Vec<Vec<u8>>,
    read_initial_report_ids: Vec<u8>,
    read_response: Option<Vec<u8>>,
    read_length: Option<usize>,
    fail_read: bool,
}

struct FakeKeyboardFactory {
    acquisitions: usize,
    state: Rc<RefCell<KeyboardState>>,
    guarded: bool,
    acquisition_error: Option<&'static str>,
}

impl FakeKeyboardFactory {
    fn ready(_legacy_status: u8) -> Self {
        let factory = Self::response(0xcc, 0x17);
        factory.state.borrow_mut().read_response =
            Some(KEYBOARD_OBSERVED_STATUS_READY_SIGNATURE.to_vec());
        factory.state.borrow_mut().read_length =
            Some(KEYBOARD_OBSERVED_STATUS_READY_SIGNATURE.len());
        factory
    }

    fn response(report_id: u8, status: u8) -> Self {
        Self {
            acquisitions: 0,
            state: Rc::new(RefCell::new(KeyboardState {
                report_id,
                status,
                ..KeyboardState::default()
            })),
            guarded: false,
            acquisition_error: None,
        }
    }

    fn permission_denied() -> Self {
        Self {
            acquisition_error: Some("keyboard_permission_denied"),
            ..Self::ready(0x8c)
        }
    }
}

impl KeyboardBackendFactory for FakeKeyboardFactory {
    fn requires_process_guard(&self) -> bool {
        self.guarded
    }

    fn acquire(
        &mut self,
        _selection: &KeyboardSelection,
    ) -> Result<Box<dyn KeyboardBackend>, BackendError> {
        self.acquisitions += 1;
        if let Some(code) = self.acquisition_error {
            return Err(BackendError::new(code));
        }
        Ok(Box::new(FakeKeyboardBackend {
            state: self.state.clone(),
        }))
    }
}

struct FakeKeyboardBackend {
    state: Rc<RefCell<KeyboardState>>,
}

impl KeyboardBackend for FakeKeyboardBackend {
    fn send_feature_report(&mut self, data: &[u8]) -> Result<usize, BackendError> {
        let mut state = self.state.borrow_mut();
        let index = state.write_count;
        state.write_count += 1;
        state.operations.push("write");
        state.feature_lengths.push(data.len());
        state.writes.push(data.to_vec());
        if state.fail_write_at == Some(index) {
            return Err(BackendError::new("fake_write_failed"));
        }
        Ok(if state.short_write_at == Some(index) {
            data.len() - 1
        } else {
            data.len()
        })
    }

    fn get_feature_report(&mut self, data: &mut [u8]) -> Result<usize, BackendError> {
        let mut state = self.state.borrow_mut();
        state.operations.push("read");
        state.feature_lengths.push(data.len());
        state.read_initial_report_ids.push(data[0]);
        if state.fail_read {
            return Err(BackendError::new("fake_read_failed"));
        }
        if let Some(response) = &state.read_response {
            let copied = response.len().min(data.len());
            data[..copied].copy_from_slice(&response[..copied]);
        } else {
            data[0] = state.report_id;
            data[2] = state.status;
        }
        Ok(state.read_length.unwrap_or(data.len()))
    }
}

#[derive(Default)]
struct AwState {
    writes: Vec<Vec<u8>>,
    fail_at: Option<usize>,
    short_at: Option<usize>,
}

#[derive(Default)]
struct FakeAwFactory {
    acquisitions: usize,
    state: Rc<RefCell<AwState>>,
    guarded: bool,
    acquisition_error: Option<&'static str>,
}

impl AwElcBackendFactory for FakeAwFactory {
    fn requires_process_guard(&self) -> bool {
        self.guarded
    }

    fn acquire(
        &mut self,
        _selection: &AwElcSelection,
    ) -> Result<Box<dyn AwElcBackend>, BackendError> {
        self.acquisitions += 1;
        if let Some(code) = self.acquisition_error {
            return Err(BackendError::new(code));
        }
        Ok(Box::new(FakeAwBackend {
            state: self.state.clone(),
        }))
    }
}

struct FakeAwBackend {
    state: Rc<RefCell<AwState>>,
}

impl AwElcBackend for FakeAwBackend {
    fn interrupt_write(&mut self, data: &[u8]) -> Result<usize, BackendError> {
        let mut state = self.state.borrow_mut();
        let index = state.writes.len();
        state.writes.push(data.to_vec());
        if state.fail_at == Some(index) {
            return Err(BackendError::new("fake_aw_failed"));
        }
        Ok(if state.short_at == Some(index) {
            data.len() - 1
        } else {
            data.len()
        })
    }
}

fn keyboard_enumeration_record(
    path: &str,
    usage_page: u16,
    usage: u16,
) -> KeyboardEnumerationRecord {
    KeyboardEnumerationRecord {
        path: path.as_bytes().to_vec(),
        is_usb: true,
        vendor_id: 0x0d62,
        product_id: 0xd2b1,
        interface_number: 0,
        manufacturer: Some("DELL Technologies".into()),
        product: Some("Keyboard".into()),
        usage_page,
        usage,
    }
}

fn supported_dmi() -> DmiIdentity {
    DmiIdentity {
        vendor: Some("Alienware".into()),
        product: Some("Alienware m16 R2".into()),
        bios_version: Some("1.21.0".into()),
        supported: true,
    }
}

fn keyboard_device() -> DeviceSummary {
    DeviceSummary {
        kind: DeviceKind::Keyboard,
        status: DeviceStatus::Found,
        vid: "0d62".into(),
        pid: "d2b1".into(),
        manufacturer: None,
        product: None,
        serial: None,
        sysfs_name: Some("3-2.3".into()),
        bus_number: Some(3),
        port_path: Some(vec![2, 3]),
        interface_number: Some("00".into()),
        hidraw: vec![HidrawInfo {
            path: "/dev/hidraw-test".into(),
            interface_number: Some("00".into()),
            node_state: HidrawNodeState::MetadataUnavailable,
            read_access: PermissionEstimate::Unknown,
            write_access: PermissionEstimate::Unknown,
            access_basis: "not_inspected",
            effective_read_access: EffectiveAccess::Unknown,
            effective_write_access: EffectiveAccess::Unknown,
            effective_access_basis: "not_inspected",
        }],
        descriptor: Some(DescriptorStatus {
            evidence: DescriptorEvidence::HashMatch,
            sha256: Some(KEYBOARD_DESCRIPTOR_SHA256.into()),
            expected_sha256: KEYBOARD_DESCRIPTOR_SHA256,
            interface_number: Some("00".into()),
            sysfs_path: Some("/sys/test/report_descriptor".into()),
        }),
    }
}

fn aw_device() -> DeviceSummary {
    DeviceSummary {
        kind: DeviceKind::Chassis,
        status: DeviceStatus::Found,
        vid: "187c".into(),
        pid: "0551".into(),
        manufacturer: None,
        product: None,
        serial: Some("bound-serial".into()),
        sysfs_name: Some("3-2.4".into()),
        bus_number: Some(3),
        port_path: Some(vec![2, 4]),
        interface_number: Some("00".into()),
        hidraw: Vec::new(),
        descriptor: None,
    }
}

#[test]
fn set_all_executes_canonical_stages_with_fresh_acquisition() {
    let color = Rgb::new(0xff, 0x69, 0xb4);
    let mut discovery = FakeDiscovery::supported();
    let mut keyboard = FakeKeyboardFactory::ready(0x17);
    let keyboard_state = keyboard.state.clone();
    let mut aw = FakeAwFactory::default();
    let aw_state = aw.state.clone();
    let outcome = execute_set_all(
        prepare_set_all(SetAllLiveIntent { color }).unwrap(),
        &mut discovery,
        &mut keyboard,
        &mut aw,
    )
    .unwrap();
    assert!(outcome.completed());
    assert_eq!(
        outcome
            .stages
            .iter()
            .map(|stage| (stage.name, stage.completed_steps))
            .collect::<Vec<_>>(),
        [
            ("keyboard", 11),
            ("aw_static_touchpad_back", 4),
            ("power_profile", 34)
        ]
    );
    assert_eq!(discovery.calls, 3);
    assert_eq!(keyboard.acquisitions, 1);
    assert_eq!(aw.acquisitions, 2);
    assert_eq!(keyboard_state.borrow().operations.len(), 11);
    let aw_state = aw_state.borrow();
    assert_eq!(aw_state.writes.len(), 38);
    let expected_static =
        api_v4::encode_static(&[LogicalColor::new(0, color), LogicalColor::new(2, color)]).unwrap();
    let expected_power = power_v4::encode_equal_color(color);
    for (actual, expected) in aw_state
        .writes
        .iter()
        .zip(expected_static.steps().iter().chain(expected_power.steps()))
    {
        assert_eq!(*actual, decode_payload(expected, 33).unwrap());
    }
}

#[test]
fn set_all_canonical_mismatch_for_each_plan_stops_before_discovery() {
    let base = prepare_set_all(SetAllLiveIntent {
        color: Rgb::new(1, 2, 3),
    })
    .unwrap();
    let mut cases = Vec::new();
    let mut keyboard_plan = base.clone();
    keyboard_plan.keyboard.canonical.assignments[0].target = "tampered".into();
    cases.push(keyboard_plan);
    let mut aw_plan = base.clone();
    aw_plan.aw_static.canonical.assignments[0].target = "tampered".into();
    cases.push(aw_plan);
    let mut power_plan = base;
    power_plan.power_profile.canonical.assignments[0].target = "tampered".into();
    cases.push(power_plan);
    for prepared in cases {
        let mut discovery = FakeDiscovery::supported();
        let mut keyboard = FakeKeyboardFactory::ready(0x17);
        let mut aw = FakeAwFactory::default();
        let error = execute_set_all(prepared, &mut discovery, &mut keyboard, &mut aw).unwrap_err();
        assert_eq!(error.code, ExecutionErrorCode::CanonicalMismatch);
        assert_eq!(discovery.calls, 0);
        assert_eq!(keyboard.acquisitions, 0);
        assert_eq!(aw.acquisitions, 0);
    }
}

#[test]
fn set_all_dual_guards_are_acquired_keyboard_then_aw_before_discovery_and_release() {
    let _test_lock = PROCESS_GUARD_TEST_LOCK.lock().unwrap();
    let prepared = prepare_set_all(SetAllLiveIntent {
        color: Rgb::new(1, 2, 3),
    })
    .unwrap();
    {
        let _keyboard_held = KeyboardExecutionGuard::try_acquire().unwrap();
        let mut discovery = FakeDiscovery::supported();
        let mut keyboard = FakeKeyboardFactory::ready(0x17);
        keyboard.guarded = true;
        let mut aw = FakeAwFactory {
            guarded: true,
            ..FakeAwFactory::default()
        };
        assert_eq!(
            execute_set_all(prepared.clone(), &mut discovery, &mut keyboard, &mut aw)
                .unwrap_err()
                .code,
            ExecutionErrorCode::Busy
        );
        assert_eq!(discovery.calls, 0);
    }
    {
        let _aw_held = AwElcExecutionGuard::try_acquire().unwrap();
        let mut discovery = FakeDiscovery::supported();
        let mut keyboard = FakeKeyboardFactory::ready(0x17);
        keyboard.guarded = true;
        let mut aw = FakeAwFactory {
            guarded: true,
            ..FakeAwFactory::default()
        };
        assert_eq!(
            execute_set_all(prepared, &mut discovery, &mut keyboard, &mut aw)
                .unwrap_err()
                .code,
            ExecutionErrorCode::Busy
        );
        assert_eq!(discovery.calls, 0);
        assert!(KeyboardExecutionGuard::try_acquire().is_ok());
    }
    assert!(KeyboardExecutionGuard::try_acquire().is_ok());
    assert!(AwElcExecutionGuard::try_acquire().is_ok());
}

#[test]
fn set_all_reports_attempted_transfer_for_first_error_and_short_but_not_acquisition() {
    let color = Rgb::new(1, 2, 3);
    for short in [false, true] {
        let mut discovery = FakeDiscovery::supported();
        let mut keyboard = FakeKeyboardFactory::ready(0x17);
        if short {
            keyboard.state.borrow_mut().short_write_at = Some(0);
        } else {
            keyboard.state.borrow_mut().fail_write_at = Some(0);
        }
        let mut aw = FakeAwFactory::default();
        let outcome = execute_set_all(
            prepare_set_all(SetAllLiveIntent { color }).unwrap(),
            &mut discovery,
            &mut keyboard,
            &mut aw,
        )
        .unwrap();
        assert!(outcome.transport_attempted());
        assert!(outcome.stages[0].transport_attempted);
        assert_eq!(outcome.stages[0].completed_steps, 0);

        let mut discovery = FakeDiscovery::supported();
        let mut keyboard = FakeKeyboardFactory::ready(0x17);
        let mut aw = FakeAwFactory::default();
        if short {
            aw.state.borrow_mut().short_at = Some(0);
        } else {
            aw.state.borrow_mut().fail_at = Some(0);
        }
        let outcome = execute_set_all(
            prepare_set_all(SetAllLiveIntent { color }).unwrap(),
            &mut discovery,
            &mut keyboard,
            &mut aw,
        )
        .unwrap();
        assert!(outcome.stages[1].transport_attempted);
        assert_eq!(outcome.stages[1].completed_steps, 0);

        let mut discovery = FakeDiscovery::supported();
        let mut keyboard = FakeKeyboardFactory::ready(0x17);
        let mut aw = FakeAwFactory::default();
        if short {
            aw.state.borrow_mut().short_at = Some(4);
        } else {
            aw.state.borrow_mut().fail_at = Some(4);
        }
        let outcome = execute_set_all(
            prepare_set_all(SetAllLiveIntent { color }).unwrap(),
            &mut discovery,
            &mut keyboard,
            &mut aw,
        )
        .unwrap();
        assert!(outcome.stages[2].transport_attempted);
        assert_eq!(outcome.stages[2].completed_steps, 0);
    }

    let mut discovery = FakeDiscovery::supported();
    let mut keyboard = FakeKeyboardFactory::permission_denied();
    let mut aw = FakeAwFactory::default();
    let outcome = execute_set_all(
        prepare_set_all(SetAllLiveIntent { color }).unwrap(),
        &mut discovery,
        &mut keyboard,
        &mut aw,
    )
    .unwrap();
    assert!(!outcome.transport_attempted());
    assert!(!outcome.stages[0].transport_attempted);
    assert_eq!(outcome.stages[0].completed_steps, 0);
    assert_eq!(
        outcome.stages[0].failure.as_ref().unwrap().code,
        ExecutionErrorCode::AcquisitionFailed
    );
}

#[test]
fn set_all_failure_stops_later_stages_without_retry() {
    let color = Rgb::new(1, 2, 3);
    let mut discovery = FakeDiscovery::supported();
    let mut keyboard = FakeKeyboardFactory::ready(0x17);
    keyboard.state.borrow_mut().fail_write_at = Some(3);
    let mut aw = FakeAwFactory::default();
    let outcome = execute_set_all(
        prepare_set_all(SetAllLiveIntent { color }).unwrap(),
        &mut discovery,
        &mut keyboard,
        &mut aw,
    )
    .unwrap();
    assert_eq!(outcome.stages[0].completed_steps, 4);
    assert!(outcome.stages[0].failure.is_some());
    assert_eq!(outcome.stages[1].completed_steps, 0);
    assert_eq!(outcome.stages[2].completed_steps, 0);
    assert_eq!(aw.acquisitions, 0);

    let mut discovery = FakeDiscovery::supported();
    let mut keyboard = FakeKeyboardFactory::ready(0x17);
    let mut aw = FakeAwFactory::default();
    aw.state.borrow_mut().fail_at = Some(4);
    let outcome = execute_set_all(
        prepare_set_all(SetAllLiveIntent { color }).unwrap(),
        &mut discovery,
        &mut keyboard,
        &mut aw,
    )
    .unwrap();
    assert_eq!(outcome.stages[0].completed_steps, 11);
    assert_eq!(outcome.stages[1].completed_steps, 4);
    assert_eq!(outcome.stages[2].completed_steps, 0);
    assert!(outcome.stages[2].failure.is_some());
    assert_eq!(aw.state.borrow().writes.len(), 5);
}

#[test]
fn validation_state_remains_mapping_derived_only() {
    assert_eq!(
        keyboard_device().descriptor.unwrap().expected_sha256,
        KEYBOARD_DESCRIPTOR_SHA256
    );
    assert_eq!(
        api_v4::encode_static(&[LogicalColor::new(0, Rgb::new(1, 2, 3))])
            .unwrap()
            .validation,
        ValidationState::MappingDerivedUnvalidated
    );
}

#[test]
fn aw_handoff_active_success_is_detach_claim_write_release_attach() {
    let selection = aw_selection();
    let state = Rc::new(RefCell::new(HandoffState::default()));
    let mut backend = acquire_aw_elc_handoff(
        &selection,
        FakeAwHandoff::new(state.clone(), HandoffConfig::active()),
    )
    .unwrap();
    backend.interrupt_write(&[0; 33]).unwrap();
    backend.finish().unwrap();
    assert_eq!(
        state.borrow().events,
        ["detach", "claim", "write", "release", "attach"]
    );
}

#[test]
fn aw_handoff_inactive_never_detaches_or_reattaches() {
    let selection = aw_selection();
    let state = Rc::new(RefCell::new(HandoffState::default()));
    let mut backend = acquire_aw_elc_handoff(
        &selection,
        FakeAwHandoff::new(state.clone(), HandoffConfig::default()),
    )
    .unwrap();
    backend.interrupt_write(&[0; 33]).unwrap();
    backend.finish().unwrap();
    assert_eq!(state.borrow().events, ["claim", "write", "release"]);
}

#[test]
fn aw_handoff_detach_failure_never_claims_or_writes() {
    let selection = aw_selection();
    let state = Rc::new(RefCell::new(HandoffState::default()));
    let error = acquire_aw_elc_handoff(
        &selection,
        FakeAwHandoff::new(
            state.clone(),
            HandoffConfig {
                active_before: true,
                fail_detach: true,
                ..HandoffConfig::default()
            },
        ),
    )
    .err()
    .unwrap();
    assert!(error.code.contains("aw_elc_detach_failed"));
    assert!(error.code.contains("remains active"));
    assert_eq!(state.borrow().events, ["detach"]);
    assert_eq!(state.borrow().driver_state_checks, 2);
}

#[test]
fn aw_handoff_detach_error_with_inactive_driver_reattaches_without_claiming() {
    let selection = aw_selection();
    let state = Rc::new(RefCell::new(HandoffState::default()));
    let error = acquire_aw_elc_handoff(
        &selection,
        FakeAwHandoff::new(
            state.clone(),
            HandoffConfig {
                active_before: true,
                fail_detach: true,
                detach_error_state: Some(DriverState::Inactive),
                ..HandoffConfig::default()
            },
        ),
    )
    .err()
    .unwrap();
    assert!(error.code.contains("aw_elc_detach_failed"));
    assert!(error.code.contains("attach cleanup succeeded"));
    assert_eq!(state.borrow().events, ["detach", "attach"]);
    assert_eq!(state.borrow().driver_state_checks, 2);
}

#[test]
fn aw_handoff_detach_error_with_unknown_driver_reports_uncertainty_without_attach() {
    let selection = aw_selection();
    let state = Rc::new(RefCell::new(HandoffState::default()));
    let error = acquire_aw_elc_handoff(
        &selection,
        FakeAwHandoff::new(
            state.clone(),
            HandoffConfig {
                active_before: true,
                fail_detach: true,
                detach_error_state: Some(DriverState::Unknown),
                ..HandoffConfig::default()
            },
        ),
    )
    .err()
    .unwrap();
    assert!(error.code.contains("driver state is unknown"));
    assert_eq!(state.borrow().events, ["detach"]);
    assert_eq!(state.borrow().driver_state_checks, 2);
}

#[test]
fn aw_handoff_claim_or_postclaim_failure_cleans_up_before_returning() {
    for config in [
        HandoffConfig {
            active_before: true,
            fail_claim: true,
            ..HandoffConfig::default()
        },
        HandoffConfig {
            active_before: true,
            invalid_postclaim: true,
            ..HandoffConfig::default()
        },
    ] {
        let selection = aw_selection();
        let state = Rc::new(RefCell::new(HandoffState::default()));
        assert!(
            acquire_aw_elc_handoff(&selection, FakeAwHandoff::new(state.clone(), config)).is_err()
        );
        let events = &state.borrow().events;
        assert_eq!(events[0..2], ["detach", "claim"]);
        assert_eq!(events.last(), Some(&"attach"));
        assert!(
            events == &["detach", "claim", "attach"]
                || events == &["detach", "claim", "release", "attach"]
        );
    }
}

#[test]
fn aw_handoff_postclaim_failure_preserves_release_and_attach_failures() {
    let selection = aw_selection();
    let state = Rc::new(RefCell::new(HandoffState::default()));
    let error = acquire_aw_elc_handoff(
        &selection,
        FakeAwHandoff::new(
            state.clone(),
            HandoffConfig {
                active_before: true,
                invalid_postclaim: true,
                fail_release: true,
                fail_attach: true,
                ..HandoffConfig::default()
            },
        ),
    )
    .err()
    .unwrap();
    assert!(error.code.contains("aw_elc_postclaim_validation_failed"));
    assert!(error.code.contains("aw_elc_release_failed"));
    assert!(error.code.contains("aw_elc_reattach_failed"));
    assert_eq!(
        state.borrow().events,
        ["detach", "claim", "release", "attach"]
    );
}

#[test]
fn aw_handoff_write_and_short_failure_cleanup_without_retry() {
    for config in [
        HandoffConfig {
            active_before: true,
            fail_write: true,
            ..HandoffConfig::default()
        },
        HandoffConfig {
            active_before: true,
            short_write: true,
            ..HandoffConfig::default()
        },
    ] {
        let selection = aw_selection();
        let state = Rc::new(RefCell::new(HandoffState::default()));
        let mut backend =
            acquire_aw_elc_handoff(&selection, FakeAwHandoff::new(state.clone(), config)).unwrap();
        let write = backend.interrupt_write(&[0; 33]);
        assert!(write.is_err() || write.unwrap() == 32);
        backend.finish().unwrap();
        assert_eq!(
            state.borrow().events,
            ["detach", "claim", "write", "release", "attach"]
        );
    }
}

#[test]
fn aw_handoff_write_and_short_failure_preserve_progress_when_cleanup_fails() {
    for config in [
        HandoffConfig {
            active_before: true,
            fail_write: true,
            fail_release: true,
            fail_attach: true,
            ..HandoffConfig::default()
        },
        HandoffConfig {
            active_before: true,
            short_write: true,
            fail_release: true,
            fail_attach: true,
            ..HandoffConfig::default()
        },
    ] {
        let mut discovery = FakeDiscovery::supported();
        let state = Rc::new(RefCell::new(HandoffState::default()));
        let mut factory = HandoffFactory {
            state: state.clone(),
            config,
        };
        let error = execute_aw_elc(
            prepare_aw_elc(vec![LogicalColor::new(0, Rgb::new(1, 2, 3))]).unwrap(),
            &mut discovery,
            &mut factory,
        )
        .unwrap_err();
        assert!(matches!(
            error.code,
            ExecutionErrorCode::BackendFailed | ExecutionErrorCode::ShortTransfer
        ));
        assert_eq!(error.completed_steps(), 0);
        assert!(error.transport_attempted);
        assert!(error.message().contains("aw_elc_release_failed"));
        assert!(error.message().contains("aw_elc_reattach_failed"));
        assert_eq!(
            state.borrow().events,
            ["detach", "claim", "write", "release", "attach"]
        );
    }
}

#[test]
fn aw_handoff_release_failure_still_attempts_reattach_and_drop_is_fallback() {
    let selection = aw_selection();
    let state = Rc::new(RefCell::new(HandoffState::default()));
    let mut backend = acquire_aw_elc_handoff(
        &selection,
        FakeAwHandoff::new(
            state.clone(),
            HandoffConfig {
                active_before: true,
                fail_release: true,
                ..HandoffConfig::default()
            },
        ),
    )
    .unwrap();
    backend.interrupt_write(&[0; 33]).unwrap();
    assert_eq!(backend.finish().unwrap_err().code, "aw_elc_release_failed");
    assert_eq!(
        state.borrow().events,
        ["detach", "claim", "write", "release", "attach"]
    );

    let both_fail_state = Rc::new(RefCell::new(HandoffState::default()));
    let mut backend = acquire_aw_elc_handoff(
        &selection,
        FakeAwHandoff::new(
            both_fail_state.clone(),
            HandoffConfig {
                active_before: true,
                fail_release: true,
                fail_attach: true,
                ..HandoffConfig::default()
            },
        ),
    )
    .unwrap();
    let error = backend.finish().unwrap_err();
    assert!(error.code.contains("aw_elc_release_failed"));
    assert!(error.code.contains("aw_elc_reattach_failed"));
    assert_eq!(
        both_fail_state.borrow().events,
        ["detach", "claim", "release", "attach"]
    );

    let fallback_state = Rc::new(RefCell::new(HandoffState::default()));
    let backend = acquire_aw_elc_handoff(
        &selection,
        FakeAwHandoff::new(fallback_state.clone(), HandoffConfig::active()),
    )
    .unwrap();
    drop(backend);
    assert_eq!(
        fallback_state.borrow().events,
        ["detach", "claim", "release", "attach"]
    );
}

#[test]
fn aw_handoff_attach_failure_after_writes_preserves_transport_progress() {
    let mut discovery = FakeDiscovery::supported();
    let state = Rc::new(RefCell::new(HandoffState::default()));
    let mut factory = HandoffFactory {
        state: state.clone(),
        config: HandoffConfig {
            active_before: true,
            fail_attach: true,
            ..HandoffConfig::default()
        },
    };
    let error = execute_aw_elc(
        prepare_aw_elc(vec![LogicalColor::new(0, Rgb::new(1, 2, 3))]).unwrap(),
        &mut discovery,
        &mut factory,
    )
    .unwrap_err();
    assert_eq!(error.code, ExecutionErrorCode::BackendFailed);
    assert_eq!(error.completed_steps(), 4);
    assert!(error.transport_attempted);
    assert!(error.message().contains("aw_elc_reattach_failed"));
    assert_eq!(
        state.borrow().events,
        ["detach", "claim", "write", "write", "write", "write", "release", "attach"]
    );
}

fn aw_selection() -> AwElcSelection {
    AwElcSelection {
        bus_number: 3,
        port_path: vec![2, 4],
        serial: Some("bound-serial".into()),
    }
}

#[derive(Clone, Default)]
struct HandoffConfig {
    active_before: bool,
    fail_detach: bool,
    detach_error_state: Option<DriverState>,
    fail_claim: bool,
    invalid_postclaim: bool,
    fail_write: bool,
    short_write: bool,
    fail_release: bool,
    fail_attach: bool,
}

impl HandoffConfig {
    fn active() -> Self {
        Self {
            active_before: true,
            ..Self::default()
        }
    }
}

#[derive(Default)]
struct HandoffState {
    events: Vec<&'static str>,
    driver_state_checks: usize,
}

struct FakeAwHandoff {
    state: Rc<RefCell<HandoffState>>,
    config: HandoffConfig,
    claimed: bool,
    detach_attempted: bool,
}

impl FakeAwHandoff {
    fn new(state: Rc<RefCell<HandoffState>>, config: HandoffConfig) -> Self {
        Self {
            state,
            config,
            claimed: false,
            detach_attempted: false,
        }
    }
}

impl AwElcHandoff for FakeAwHandoff {
    fn evidence(&mut self, _selection: &AwElcSelection) -> Result<AwOpenedEvidence, BackendError> {
        let mut evidence = valid_aw_evidence();
        if self.claimed && self.config.invalid_postclaim {
            evidence.interface.protocol = 1;
        }
        Ok(evidence)
    }

    fn driver_state(&mut self) -> DriverState {
        self.state.borrow_mut().driver_state_checks += 1;
        if self.detach_attempted && self.config.fail_detach {
            self.config
                .detach_error_state
                .unwrap_or(DriverState::Active)
        } else if self.claimed || !self.config.active_before {
            DriverState::Inactive
        } else {
            DriverState::Active
        }
    }

    fn detach_kernel_driver(&mut self) -> Result<(), BackendError> {
        self.state.borrow_mut().events.push("detach");
        self.detach_attempted = true;
        if self.config.fail_detach {
            Err(BackendError::new("aw_elc_detach_failed"))
        } else {
            Ok(())
        }
    }

    fn claim_interface(&mut self) -> Result<(), BackendError> {
        self.state.borrow_mut().events.push("claim");
        if self.config.fail_claim {
            Err(BackendError::new("aw_elc_claim_failed"))
        } else {
            self.claimed = true;
            Ok(())
        }
    }

    fn release_interface(&mut self) -> Result<(), BackendError> {
        self.state.borrow_mut().events.push("release");
        if self.config.fail_release {
            Err(BackendError::new("aw_elc_release_failed"))
        } else {
            Ok(())
        }
    }

    fn attach_kernel_driver(&mut self) -> Result<(), BackendError> {
        self.state.borrow_mut().events.push("attach");
        if self.config.fail_attach {
            Err(BackendError::new("aw_elc_reattach_failed"))
        } else {
            Ok(())
        }
    }

    fn interrupt_write(&mut self, _data: &[u8]) -> Result<usize, BackendError> {
        self.state.borrow_mut().events.push("write");
        if self.config.fail_write {
            Err(BackendError::new("fake_aw_write_failed"))
        } else if self.config.short_write {
            Ok(32)
        } else {
            Ok(33)
        }
    }
}

struct HandoffFactory {
    state: Rc<RefCell<HandoffState>>,
    config: HandoffConfig,
}

impl AwElcBackendFactory for HandoffFactory {
    fn acquire(
        &mut self,
        selection: &AwElcSelection,
    ) -> Result<Box<dyn AwElcBackend>, BackendError> {
        Ok(Box::new(acquire_aw_elc_handoff(
            selection,
            FakeAwHandoff::new(self.state.clone(), self.config.clone()),
        )?))
    }
}
