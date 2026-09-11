use super::{padded_hex, EncodeError, LogicalColor};
use crate::model::{
    PacketStep, PlanAssignment, PlanAssumption, ProtocolFamily, ProtocolPlan, TransferKind,
    ValidationState,
};
use crate::targets::keyboard_target_by_id;
use std::collections::HashSet;

const BUFFER_LENGTH: usize = 64;
const RECORDS_PER_FRAME: usize = 15;

pub(crate) fn status_query_buffers() -> ([u8; BUFFER_LENGTH], [u8; BUFFER_LENGTH]) {
    let mut query = [0_u8; BUFFER_LENGTH];
    query[..2].copy_from_slice(&[0xcc, 0x93]);
    let mut response = [0_u8; BUFFER_LENGTH];
    response[0] = 0xcc;
    (query, response)
}

// Packet constants are translated from the MIT-licensed T-Troll/alienfx-tools
// implementation and m16R2 (US) mapping. This module only constructs data.
pub fn encode_static(assignments: &[LogicalColor]) -> Result<ProtocolPlan, EncodeError> {
    let assignments = validate_and_sort(assignments)?;
    let mut steps = vec![
        write_step("reset", &[0xcc, 0x94]),
        write_step("query_status", &[0xcc, 0x93]),
        PacketStep {
            name: "read_status",
            transfer: TransferKind::HidFeatureReadIntent,
            caller_buffer_length: BUFFER_LENGTH,
            on_wire_length: BUFFER_LENGTH,
            payload_hex: Some(padded_hex(&[0xcc], BUFFER_LENGTH)),
        },
    ];

    for frame in assignments.chunks(RECORDS_PER_FRAME) {
        let mut payload = vec![0xcc, 0x8c, 0x02, 0x00];
        for assignment in frame {
            let encoded_id = assignment
                .logical_id
                .checked_add(1)
                .ok_or(EncodeError::EncodedIdOutOfRange(assignment.logical_id))?;
            payload.extend_from_slice(&[
                encoded_id,
                assignment.color.r,
                assignment.color.g,
                assignment.color.b,
            ]);
        }
        steps.push(write_step("set_color", &payload));
    }
    steps.push(write_step("loop", &[0xcc, 0x8c, 0x13]));
    steps.push(write_step("update", &[0xcc, 0x8b, 0x01, 0xff]));

    let plan_assignments = assignments
        .iter()
        .map(|assignment| {
            let target = keyboard_target_by_id(assignment.logical_id)
                .expect("validated keyboard target must remain available");
            PlanAssignment {
                target: target.name.to_string(),
                logical_id: assignment.logical_id,
                encoded_id: assignment.logical_id.checked_add(1),
                color: assignment.color,
                color_hex: assignment.color.hex(),
                validation: target.validation,
            }
        })
        .collect();

    Ok(ProtocolPlan {
        family: ProtocolFamily::AlienFxApiV5,
        validation: ValidationState::MappingDerivedUnvalidated,
        assignments: plan_assignments,
        assumptions: vec![PlanAssumption {
            name: "transport_not_executed",
            state: ValidationState::MappingDerivedUnvalidated,
            detail: "64-byte HID feature transfers are modeled only; no device I/O occurs.",
        }],
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
        if assignment.logical_id.checked_add(1).is_none() {
            return Err(EncodeError::EncodedIdOutOfRange(assignment.logical_id));
        }
        if keyboard_target_by_id(assignment.logical_id).is_none() {
            return Err(EncodeError::UnknownLogicalId(assignment.logical_id));
        }
        if !seen.insert(assignment.logical_id) {
            return Err(EncodeError::DuplicateLogicalId(assignment.logical_id));
        }
    }
    sorted.sort_by_key(|assignment| assignment.logical_id);
    Ok(sorted)
}

fn write_step(name: &'static str, prefix: &[u8]) -> PacketStep {
    PacketStep {
        name,
        transfer: TransferKind::HidFeatureWrite,
        caller_buffer_length: BUFFER_LENGTH,
        on_wire_length: BUFFER_LENGTH,
        payload_hex: Some(padded_hex(prefix, BUFFER_LENGTH)),
    }
}
