use alienrgb::model::{CommandKind, DeviceKind, ListReport};
use alienrgb::presentation::to_json;
use alienrgb::profile::{
    descriptor_evidence, match_system, DescriptorEvidence, CONFIRMED_BIOS_VERSION,
};

#[test]
fn matches_only_confirmed_m16_r2_dmi_identity() {
    assert!(match_system("Alienware", "Alienware m16 R2"));
    assert!(match_system("alienware", "alienware m16 r2"));
    assert!(!match_system("Alienware", "Alienware m16 R1"));
    assert!(!match_system("Dell Inc.", "Alienware m16 R2"));
}

#[test]
fn confirmed_hardware_profile_uses_one_bios_constant() {
    assert_eq!(CONFIRMED_BIOS_VERSION, "1.21.0");
}

#[test]
fn recognizes_confirmed_keyboard_descriptor_hash() {
    let descriptor = confirmed_keyboard_descriptor_shape();
    let evidence = descriptor_evidence(
        &descriptor,
        Some("b552e49c3a7ed64aba7c2f1889a2563b17bf80ad270430ad4d5954237c9bb24f"),
    );
    assert_eq!(evidence, DescriptorEvidence::HashMatch);
}

#[test]
fn distinguishes_compatible_signature_from_hash_match() {
    let descriptor = confirmed_keyboard_descriptor_shape();
    let evidence = descriptor_evidence(
        &descriptor,
        Some("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"),
    );
    assert_eq!(evidence, DescriptorEvidence::CompatibleSignature);
}

#[test]
fn rejects_descriptor_without_64_byte_vendor_feature_report() {
    let descriptor = [
        0x06, 0x89, 0xff, // Usage Page (0xff89)
        0x09, 0xcc, // Usage (0xcc)
        0x85, 0xcc, // Report ID (0xcc)
        0x75, 0x08, // Report Size (8)
        0x95, 0x3e, // Report Count (62, not 63)
        0xb1, 0x02, // Feature
    ];
    assert_eq!(
        descriptor_evidence(&descriptor, None),
        DescriptorEvidence::Mismatch
    );
}

#[test]
fn rejects_vendor_usage_when_feature_is_outside_its_collection() {
    let descriptor = [
        0x06, 0x89, 0xff, // Usage Page (0xff89)
        0x09, 0xcc, // Usage (0xcc)
        0xa1, 0x01, // Collection
        0xc0, // End Collection
        0x85, 0xcc, // Report ID (0xcc)
        0x75, 0x08, // Report Size (8)
        0x95, 0x3f, // Report Count (63)
        0xb1, 0x02, // Unrelated feature
    ];
    assert!(!alienrgb::profile::has_keyboard_feature_signature(
        &descriptor
    ));
}

#[test]
fn applies_hid_global_push_and_pop_to_feature_signature() {
    let descriptor = [
        0x06, 0x89, 0xff, // Usage Page (0xff89)
        0x09, 0xcc, // Usage (0xcc)
        0xa1, 0x01, // Collection
        0x85, 0xcc, // Report ID (0xcc)
        0x75, 0x08, // Report Size (8)
        0x95, 0x3f, // Report Count (63)
        0xa4, // Push globals
        0x95, 0x01, // Temporary Report Count (1)
        0xb4, // Pop globals
        0xb1, 0x02, // Feature using restored globals
        0xc0, // End Collection
    ];
    assert!(alienrgb::profile::has_keyboard_feature_signature(
        &descriptor
    ));
}

#[test]
fn rejects_matching_feature_in_unbalanced_collection() {
    let mut descriptor = confirmed_keyboard_descriptor_shape();
    descriptor.pop(); // Remove End Collection
    assert!(!alienrgb::profile::has_keyboard_feature_signature(
        &descriptor
    ));
}

#[test]
fn rejects_unbalanced_global_stack() {
    let mut descriptor = confirmed_keyboard_descriptor_shape();
    descriptor.insert(descriptor.len() - 1, 0xa4); // PUSH without POP after Feature
    assert_eq!(&descriptor[13..], &[0xb1, 0x02, 0xa4, 0xc0]);
    assert!(!alienrgb::profile::has_keyboard_feature_signature(
        &descriptor
    ));
}

#[test]
fn list_json_has_stable_envelope_and_device_kind() {
    let report = ListReport {
        schema_version: 1,
        command: CommandKind::List,
        supported_system: true,
        devices: vec![alienrgb::model::DeviceSummary::missing(
            DeviceKind::Keyboard,
        )],
    };

    let value: serde_json::Value = serde_json::from_str(&to_json(&report).unwrap()).unwrap();
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["command"], "list");
    assert_eq!(value["devices"][0]["kind"], "keyboard");
    assert_eq!(value["devices"][0]["status"], "missing");
}

fn confirmed_keyboard_descriptor_shape() -> Vec<u8> {
    vec![
        0x06, 0x89, 0xff, // Usage Page (0xff89)
        0x09, 0xcc, // Usage (0xcc)
        0xa1, 0x01, // Collection
        0x85, 0xcc, // Report ID (0xcc)
        0x75, 0x08, // Report Size (8)
        0x95, 0x3f, // Report Count (63)
        0xb1, 0x02, // Feature
        0xc0, // End Collection
    ]
}
