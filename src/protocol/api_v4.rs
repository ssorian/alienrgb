use super::{padded_hex, EncodeError, LogicalColor, Rgb};
use crate::model::{
    PacketStep, PlanAssignment, PlanAssumption, ProtocolFamily, ProtocolPlan, TransferKind,
    ValidationState,
};
use crate::targets::{chassis_target_by_id, static_color_validation};
use std::collections::{BTreeMap, HashSet};

const PAYLOAD_LENGTH: usize = 33;

// Direct-libusb payload constants are translated from the MIT-licensed
// T-Troll/alienfx-tools implementation. No report-ID prefix is added here.
pub fn encode_static(assignments: &[LogicalColor]) -> Result<ProtocolPlan, EncodeError> {
    let assignments = validate_and_sort(assignments)?;
    let mut steps = vec![
        output_step("remove", &[0x03, 0x21, 0x00, 0x04, 0xff, 0xff]),
        output_step("start", &[0x03, 0x21, 0x00, 0x01, 0xff, 0xff]),
    ];

    let mut by_color: BTreeMap<Rgb, Vec<u8>> = BTreeMap::new();
    for assignment in &assignments {
        by_color
            .entry(assignment.color)
            .or_default()
            .push(assignment.logical_id);
    }
    for (color, ids) in by_color {
        let mut payload = vec![0x03, 0x27, color.r, color.g, color.b, 0x00, ids.len() as u8];
        payload.extend_from_slice(&ids);
        steps.push(output_step("set_color", &payload));
    }
    steps.push(output_step(
        "finish_play",
        &[0x03, 0x21, 0x00, 0x03, 0xff, 0xff],
    ));

    let plan_assignments: Vec<PlanAssignment> = assignments
        .iter()
        .map(|assignment| {
            let target = chassis_target_by_id(assignment.logical_id)
                .expect("validated chassis target must remain available");
            PlanAssignment {
                target: target.name.to_string(),
                logical_id: assignment.logical_id,
                encoded_id: None,
                color: assignment.color,
                color_hex: assignment.color.hex(),
                validation: static_color_validation(target, assignment.color),
            }
        })
        .collect();

    let first_validation = plan_assignments[0].validation;
    let has_exact_validation = plan_assignments.iter().any(|assignment| {
        matches!(
            assignment.validation,
            ValidationState::ExactTouchpadStaticRedLiveValidated
                | ValidationState::ExactBackStaticRedLiveValidated
        )
    });
    let validation = if plan_assignments
        .iter()
        .all(|assignment| assignment.validation == first_validation)
    {
        first_validation
    } else if has_exact_validation {
        ValidationState::MixedChassisTargetEvidence
    } else {
        ValidationState::MappingDerivedUnvalidated
    };

    Ok(ProtocolPlan {
        family: ProtocolFamily::AlienFxApiV4,
        validation,
        assignments: plan_assignments,
        assumptions: vec![
            PlanAssumption {
                name: "exact_touchpad_observed_execution",
                state: ValidationState::ExactTouchpadStaticRedLiveValidated,
                detail: "Touchpad/haptic logical ID 0 has one exact static #ff0000 record on Alienware m16 R2 BIOS 1.21.0 with AW-ELC 187c:0551: remove/start/set_color/finish_play each transferred 33 bytes once and the touchpad was visually confirmed red.",
            },
            PlanAssumption {
                name: "exact_back_observed_execution",
                state: ValidationState::ExactBackStaticRedLiveValidated,
                detail: "Back/chassis logical ID 2 has one separate exact static #ff0000 record on Alienware m16 R2 BIOS 1.21.0 with AW-ELC 187c:0551: remove/start/set_color/finish_play each transferred 33 bytes once and only rear lighting was confirmed red.",
            },
            PlanAssumption {
                name: "unvalidated_assignments",
                state: ValidationState::MappingDerivedUnvalidated,
                detail: "Power logical ID 4 remains mapping-derived and unvalidated for ordinary static/address semantics. Any touchpad or back color other than #ff0000, and every other mode, firmware, or model assignment, is also unvalidated; no readback, persistence, automatic restore, or broader compatibility is claimed.",
            },
        ],
        steps,
    })
}

fn validate_and_sort(assignments: &[LogicalColor]) -> Result<Vec<LogicalColor>, EncodeError> {
    if assignments.is_empty() {
        return Err(EncodeError::EmptyAssignments);
    }
    let mut seen = HashSet::new();
    let mut sorted = assignments.to_vec();
    for assignment in &sorted {
        if chassis_target_by_id(assignment.logical_id).is_none() {
            return Err(EncodeError::UnknownLogicalId(assignment.logical_id));
        }
        if !seen.insert(assignment.logical_id) {
            return Err(EncodeError::DuplicateLogicalId(assignment.logical_id));
        }
    }
    sorted.sort_by_key(|assignment| assignment.logical_id);
    Ok(sorted)
}

fn output_step(name: &'static str, prefix: &[u8]) -> PacketStep {
    PacketStep {
        name,
        transfer: TransferKind::UsbOutput,
        caller_buffer_length: PAYLOAD_LENGTH,
        on_wire_length: PAYLOAD_LENGTH,
        payload_hex: Some(padded_hex(prefix, PAYLOAD_LENGTH)),
    }
}
