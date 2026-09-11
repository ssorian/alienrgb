use super::{padded_hex, Rgb};
use crate::model::{
    PacketStep, PlanAssignment, PlanAssumption, ProtocolFamily, ProtocolPlan, TransferKind,
    ValidationState,
};

const PAYLOAD_LENGTH: usize = 33;
const POWER_LOGICAL_ID: u8 = 4;

/// One canonical AW-ELC API v4 power state, in upstream SetPowerAction order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PowerState {
    pub id: u8,
    pub name: &'static str,
    pub packet_count: usize,
}

pub const STATES: [PowerState; 6] = [
    PowerState {
        id: 0x5b,
        name: "ac_sleep",
        packet_count: 6,
    },
    PowerState {
        id: 0x5c,
        name: "ac_on",
        packet_count: 5,
    },
    PowerState {
        id: 0x5d,
        name: "charging",
        packet_count: 6,
    },
    PowerState {
        id: 0x5e,
        name: "battery_sleep",
        packet_count: 6,
    },
    PowerState {
        id: 0x5f,
        name: "battery_on",
        packet_count: 5,
    },
    PowerState {
        id: 0x60,
        name: "battery_critical",
        packet_count: 5,
    },
];

#[derive(Clone, Copy)]
enum ActionKind {
    Color,
    Pulse,
    Power,
}

#[derive(Clone, Copy)]
struct Action {
    kind: ActionKind,
    color: Rgb,
}

/// Builds a pure, direct-libusb power-profile plan. It cannot perform transport.
///
/// Translated from SetPowerAction and SetV4Action in MIT-licensed
/// T-Troll/alienfx-tools commit 52713b238066d1343a492018ded546ff751cfcd4.
pub fn encode_equal_color(color: Rgb) -> ProtocolPlan {
    let mut steps = Vec::with_capacity(34);
    for (state_index, state) in STATES.iter().enumerate() {
        let names = STEP_NAMES[state_index];
        steps.push(output_step(
            names[0],
            &[0x03, 0x22, 0x00, 0x04, 0x00, state.id],
        ));
        steps.push(output_step(
            names[1],
            &[0x03, 0x22, 0x00, 0x01, 0x00, state.id],
        ));
        steps.push(output_step(
            names[2],
            &[0x03, 0x23, 0x01, 0x00, 0x01, POWER_LOGICAL_ID],
        ));
        for (packet_index, records) in state_actions(state.id, color).chunks(3).enumerate() {
            let mut payload = vec![0x03, 0x24];
            for action in records {
                payload.extend_from_slice(&encode_action(*action));
            }
            steps.push(output_step(names[3 + packet_index], &payload));
        }
        steps.push(output_step(
            names[state.packet_count - 1],
            &[0x03, 0x22, 0x00, 0x02, 0x00, state.id],
        ));
    }
    steps.push(output_step(
        "final_play",
        &[0x03, 0x21, 0x00, 0x05, 0xff, 0xff],
    ));
    debug_assert_eq!(steps.len(), 34);

    ProtocolPlan {
        family: ProtocolFamily::AlienFxApiV4,
        validation: ValidationState::MappingDerivedUnvalidated,
        assignments: vec![PlanAssignment {
            target: "power".into(),
            logical_id: POWER_LOGICAL_ID,
            encoded_id: None,
            color,
            color_hex: color.hex(),
            validation: ValidationState::MappingDerivedUnvalidated,
        }],
        assumptions: vec![
            PlanAssumption {
                name: "power_id_mapping",
                state: ValidationState::MappingDerivedUnvalidated,
                detail: "Alienware m16R2 mapping marks Power logical ID 4 with the POWER flag. One exact equal-red profile completed and battery-on was observed red; the other five states and individual state semantics remain visually unvalidated.",
            },
            PlanAssumption {
                name: "equal_ac_dc_color",
                state: ValidationState::MappingDerivedUnvalidated,
                detail: "This first slice intentionally supplies one common color as both upstream AC and battery colors across all six power states.",
            },
        ],
        steps,
    }
}

/// Backward-compatible pure-plan entry point.
pub fn encode(color: Rgb) -> ProtocolPlan {
    encode_equal_color(color)
}

fn state_actions(state_id: u8, color: Rgb) -> Vec<Action> {
    let power = Action {
        kind: ActionKind::Power,
        color,
    };
    let zero = Action {
        kind: ActionKind::Power,
        color: Rgb::new(0, 0, 0),
    };
    match state_id {
        0x5b => vec![power, power, power, zero],
        0x5c => vec![
            Action {
                kind: ActionKind::Color,
                color,
            },
            power,
            power,
        ],
        0x5d => vec![power, power, power, power],
        0x5e => vec![power, power, power, zero],
        0x5f => vec![
            Action {
                kind: ActionKind::Color,
                color,
            },
            power,
            power,
        ],
        0x60 => vec![
            Action {
                kind: ActionKind::Pulse,
                color,
            },
            power,
            power,
        ],
        _ => unreachable!("state IDs are fixed by STATES"),
    }
}

fn encode_action(action: Action) -> [u8; 8] {
    let (encoded_type, opcode, tempo) = match action.kind {
        ActionKind::Color => (0x00, 0xd0, 0xfa),
        ActionKind::Pulse => (0x01, 0xdc, 0x64),
        ActionKind::Power => (0x02, 0xe8, 0x64),
    };
    [
        encoded_type,
        0x03,
        opcode,
        0x00,
        tempo,
        action.color.r(),
        action.color.g(),
        action.color.b(),
    ]
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

const STEP_NAMES: [[&str; 6]; 6] = [
    [
        "state_ac_sleep_remove",
        "state_ac_sleep_start",
        "state_ac_sleep_color_select",
        "state_ac_sleep_action_1",
        "state_ac_sleep_action_2",
        "state_ac_sleep_finish",
    ],
    [
        "state_ac_on_remove",
        "state_ac_on_start",
        "state_ac_on_color_select",
        "state_ac_on_action_1",
        "state_ac_on_finish",
        "state_ac_on_unused",
    ],
    [
        "state_charging_remove",
        "state_charging_start",
        "state_charging_color_select",
        "state_charging_action_1",
        "state_charging_action_2",
        "state_charging_finish",
    ],
    [
        "state_battery_sleep_remove",
        "state_battery_sleep_start",
        "state_battery_sleep_color_select",
        "state_battery_sleep_action_1",
        "state_battery_sleep_action_2",
        "state_battery_sleep_finish",
    ],
    [
        "state_battery_on_remove",
        "state_battery_on_start",
        "state_battery_on_color_select",
        "state_battery_on_action_1",
        "state_battery_on_finish",
        "state_battery_on_unused",
    ],
    [
        "state_battery_critical_remove",
        "state_battery_critical_start",
        "state_battery_critical_color_select",
        "state_battery_critical_action_1",
        "state_battery_critical_finish",
        "state_battery_critical_unused",
    ],
];
