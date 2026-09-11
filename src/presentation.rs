use crate::model::{
    DoctorReport, DryRunReport, EffectiveAccess, ExecutionMode, FindingLevel, HidrawNodeState,
    InfoReport, KeyboardStatusReport, ListReport, LiveWriteReport, PermissionEstimate,
    PowerProfileReport, SetAllReport, ValidationState, ZonesReport,
};
use serde::Serialize;

pub fn to_json<T: Serialize>(report: &T) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(report)
}

pub fn list_human(report: &ListReport) -> String {
    if !report.supported_system {
        return "No devices reported: DMI does not match the supported Alienware m16 R2 profile.\n"
            .into();
    }
    if report.devices.is_empty() {
        return "No known Alienware m16 R2 RGB devices found.\n".into();
    }
    report
        .devices
        .iter()
        .map(|device| {
            format!(
                "{:?} {}:{} at {} (hidraw: {})\n",
                device.kind,
                device.vid,
                device.pid,
                device.sysfs_name.as_deref().unwrap_or("unknown"),
                if device.hidraw.is_empty() {
                    "no"
                } else {
                    "yes"
                }
            )
        })
        .collect()
}

pub fn info_human(report: &InfoReport) -> String {
    let mut output = format!(
        "System: {} / {} (BIOS {}) — {}\n",
        report.dmi.vendor.as_deref().unwrap_or("unknown"),
        report.dmi.product.as_deref().unwrap_or("unknown"),
        report.dmi.bios_version.as_deref().unwrap_or("unknown"),
        if report.dmi.supported {
            "supported"
        } else {
            "unsupported"
        }
    );
    for device in &report.devices {
        output.push_str(&format!(
            "- {:?} {}:{}: {:?}, hidraw={}\n",
            device.kind,
            device.vid,
            device.pid,
            device.status,
            device.hidraw.len()
        ));
        if let Some(descriptor) = &device.descriptor {
            output.push_str(&format!("  descriptor: {:?}", descriptor.evidence));
            if let Some(hash) = &descriptor.sha256 {
                output.push_str(&format!(" ({hash})"));
            }
            output.push('\n');
        }
        for hidraw in &device.hidraw {
            output.push_str(&format!(
                "  {} node={} mode_bits(read={}, write={}, basis={}) effective(read={}, write={}, basis={})\n",
                hidraw.path,
                node_text(hidraw.node_state),
                access_text(hidraw.read_access),
                access_text(hidraw.write_access),
                hidraw.access_basis,
                effective_access_text(hidraw.effective_read_access),
                effective_access_text(hidraw.effective_write_access),
                hidraw.effective_access_basis
            ));
        }
    }
    for capability in &report.capabilities {
        output.push_str(&format!(
            "- {:?} capability: {}\n  validation: {}\n",
            capability.kind, capability.write_support, capability.validation_summary
        ));
    }
    output.push_str(
        "Safety: diagnostics are read-only; only explicitly confirmed experimental AW-ELC apply is live-capable.\n",
    );
    output
}

pub fn zones_human(report: &ZonesReport) -> String {
    let mut output = String::from(
        "Known logical RGB targets behind two USB controllers (not separate USB devices):\n",
    );
    for zone in &report.zones {
        let aliases = if zone.aliases.is_empty() {
            String::new()
        } else {
            format!(" aliases={}", zone.aliases.join(","))
        };
        output.push_str(&format!(
            "- {:?} {} id={} target={}{} validation={:?} detail={}\n",
            zone.device,
            zone.usb_controller,
            zone.logical_id,
            zone.target,
            aliases,
            zone.validation,
            zone.validation_detail
        ));
        if let Some(record) = zone.known_validation {
            output.push_str(&format!(
                "  known_validation: mode={:?} color={} profile={} bios={} controller={} target={} logical_id={} evidence={}\n",
                record.mode,
                record.color,
                record.device_profile,
                record.bios_version,
                record.controller,
                record.target,
                record.logical_id,
                record.evidence
            ));
        }
    }
    output
}

pub fn dry_run_human(report: &DryRunReport) -> String {
    let mut output = String::from("DRY RUN — NO HARDWARE TRANSPORT\n");
    output.push_str(&format!(
        "Device: {:?}; validation={:?}\n",
        report.requested_device, report.plan.validation
    ));
    for assignment in &report.plan.assignments {
        output.push_str(&format!(
            "- target={} logical_id={} color={} validation={:?}\n",
            assignment.target, assignment.logical_id, assignment.color_hex, assignment.validation
        ));
    }
    for assumption in &report.plan.assumptions {
        output.push_str(&format!(
            "  assumption={} state={:?} detail={}\n",
            assumption.name, assumption.state, assumption.detail
        ));
    }
    for step in &report.plan.steps {
        output.push_str(&format!(
            "  step={} transfer={:?} caller={} on_wire={} hex={}\n",
            step.name,
            step.transfer,
            step.caller_buffer_length,
            step.on_wire_length,
            step.payload_hex.as_deref().unwrap_or("none")
        ));
    }
    if report.requested_device == crate::model::DeviceKind::Chassis {
        output.push_str("Chassis framing: 33-byte direct-libusb payload; target validation is assignment-specific.\n");
    }
    output
}

pub fn power_profile_human(report: &PowerProfileReport) -> String {
    let mut output = match report.mode {
        ExecutionMode::DryRun => String::from("POWER PROFILE DRY RUN — NO HARDWARE TRANSPORT\n"),
        ExecutionMode::Apply => String::from("POWER PROFILE WRITE COMPLETED\n"),
    };
    output.push_str(&format!(
        "Profile: {}; validation={:?}\nController: {}; Power logical ID: {}; color={} for both AC and DC\nPackets: {}; transport_performed={}\n",
        report.profile_kind,
        report.validation,
        report.controller,
        report.power_logical_id,
        report.color,
        report.packet_count,
        report.transport_performed
    ));
    for state in &report.states {
        output.push_str(&format!(
            "- state_id=0x{:02x} state={} packets={}\n",
            state.id, state.name, state.packet_count
        ));
    }
    for step in &report.plan.steps {
        output.push_str(&format!(
            "  step={} transfer={:?} caller={} on_wire={} hex={}\n",
            step.name,
            step.transfer,
            step.caller_buffer_length,
            step.on_wire_length,
            step.payload_hex.as_deref().unwrap_or("none")
        ));
    }
    if report.mode == ExecutionMode::Apply {
        output.push_str(&format!(
            "completed_steps={}\n",
            report.executed_steps.len()
        ));
    }
    output.push_str(
        "persistence=unknown_power_profile_state; restore unavailable; readback unavailable; no status polling.\n",
    );
    output
}

pub fn set_all_human(report: &SetAllReport) -> String {
    let mut output = match report.mode {
        ExecutionMode::DryRun => String::from("SET-ALL DRY RUN — NO HARDWARE TRANSPORT\n"),
        ExecutionMode::Apply => String::from("SET-ALL COMPOUND WRITE COMPLETED\n"),
    };
    output.push_str(&format!(
        "Color: {}; order: keyboard -> aw_static_touchpad_back -> power_profile\nPlans: keyboard=85 assignments/6 color frames/11 operations; AW static IDs [0,2]=4 writes; power=6 states/34 writes; total=49 transport steps\n",
        report.color
    ));
    if report.validation == ValidationState::ExactSetAllHotPinkLiveValidated {
        output.push_str(&format!(
            "validation={:?}; exact global Hot Pink run completed 49/49 once without retry and visibly confirmed keyboard, touchpad, rear, and power; power scope=current AC-charging state only; individual keyboard ID identity is not generalized.\n",
            report.validation
        ));
    } else {
        output.push_str(&format!(
            "validation={:?}; {}\n",
            report.validation, report.validation_evidence
        ));
    }
    if !report.stages.is_empty() {
        for stage in &report.stages {
            output.push_str(&format!(
                "- {}: {:?} {}/{} steps transport_attempted={}",
                stage.stage,
                stage.status,
                stage.completed_steps,
                stage.expected_steps,
                stage.transport_attempted
            ));
            if let Some(failure) = &stage.failure {
                output.push_str(&format!(
                    " failure={} step={} message={}",
                    failure.code,
                    failure.step.as_deref().unwrap_or("preflight"),
                    failure.message
                ));
            }
            output.push('\n');
        }
    }
    output.push_str(&format!(
        "transport_attempted={}; transport_performed={}; rollback={}; persistence={}; readback={}\n",
        report.transport_attempted,
        report.transport_performed,
        report.rollback,
        report.persistence,
        report.readback
    ));
    output
}

pub fn keyboard_status_human(report: &KeyboardStatusReport) -> String {
    format!(
        "EXPERIMENTAL KEYBOARD STATUS CAPTURE COMPLETED\nQuery write length: {}; response length: {}; response hex: {}\nColor frames sent: no; this is protocol status grammar capture, not RGB readback; persistence: not claimed.\n",
        report.query_write_length, report.response_length, report.response_hex
    )
}

pub fn live_write_human(report: &LiveWriteReport) -> String {
    let mut output = match report.requested_device {
        crate::model::DeviceKind::Keyboard => {
            String::from("EXPERIMENTAL KEYBOARD WRITE COMPLETED\n")
        }
        crate::model::DeviceKind::Chassis => String::from("EXPERIMENTAL AW-ELC WRITE COMPLETED\n"),
    };
    if report.requested_device == crate::model::DeviceKind::Chassis || report.target_count == 1 {
        output.push_str(&format!(
            "Target: {} (requested {}); color: {}\n",
            report.resolved_target, report.requested_target, report.color
        ));
    } else if report.requested_target.eq_ignore_ascii_case("all") {
        output.push_str(&format!(
            "Targets: all known keyboard targets (count {}); common color: {}\n",
            report.target_count, report.color
        ));
    } else {
        output.push_str(&format!(
            "Targets: {} (requested {}; count {}); common color: {}\n",
            report.resolved_target, report.requested_target, report.target_count, report.color
        ));
    }
    if report.requested_device == crate::model::DeviceKind::Keyboard {
        let operations = 5 + report.set_color_frames;
        output.push_str(&format!(
            "set_color frames: {}; HID operations: {}; ordinary timeout estimate: about {} seconds plus scheduling/cancellation delay.\n",
            report.set_color_frames,
            operations,
            operations * 5
        ));
        output.push_str("A failure after an earlier color frame can leave a partial applied state; no restore, readback, or persistence is claimed.\n");
    }
    for step in &report.executed_steps {
        output.push_str(&format!(
            "- step={} transferred_length={}\n",
            step.name, step.transferred_length
        ));
    }
    output.push_str("state restore available: no; persistence: not claimed.\n");
    output
}

pub fn doctor_human(report: &DoctorReport) -> String {
    let mut output = String::new();
    for finding in &report.findings {
        let marker = match finding.level {
            FindingLevel::Ok => "OK",
            FindingLevel::Warning => "WARN",
            FindingLevel::Error => "ERROR",
        };
        output.push_str(&format!(
            "[{marker}] {}: {}\n",
            finding.code, finding.message
        ));
        if let Some(action) = &finding.action {
            output.push_str(&format!("       Action: {action}\n"));
        }
    }
    output.push_str(&format!(
        "Diagnostic readiness: {}. Keyboard live-write readiness: {}. Chassis live-write readiness: {}. Combined write readiness: {}.\n",
        if report.ready_for_diagnostics {
            "ready"
        } else {
            "not ready"
        },
        if report.keyboard_ready_for_live_write {
            "technically ready (confirmed BIOS, descriptor, interface-00 node, and effective read/write access)"
        } else {
            "not ready"
        },
        if report.chassis_ready_for_live_write {
            "technically ready before acquisition"
        } else {
            "not ready"
        },
        if report.ready_for_writes {
            "both known controller profiles technically ready"
        } else {
            "not ready"
        }
    ));
    output.push_str("Physical validation: keyboard one-, two-, and six-frame static red plus whole-keyboard coverage were observed on the exact profile; only Escape/F1/W/Space were individually discriminated. Chassis validation covers touchpad static red, back static red, and the dedicated equal-color power-profile #ff0000 only during battery-on/discharging; all other colors, effects, profiles, the other five power states, and ordinary power static/address semantics remain unvalidated. Technical readiness does not itself validate a physical target, and AW-ELC endpoint/driver/access checks occur only during acquisition.\n");
    output
}

fn effective_access_text(value: EffectiveAccess) -> &'static str {
    match value {
        EffectiveAccess::Allowed => "allowed",
        EffectiveAccess::Denied => "denied",
        EffectiveAccess::Unknown => "unknown",
        EffectiveAccess::Missing => "missing",
    }
}

fn access_text(value: PermissionEstimate) -> &'static str {
    match value {
        PermissionEstimate::AllowedByModeBits => "allowed_by_mode_bits",
        PermissionEstimate::DeniedByModeBits => "denied_by_mode_bits",
        PermissionEstimate::Unknown => "unknown",
    }
}

fn node_text(value: HidrawNodeState) -> &'static str {
    match value {
        HidrawNodeState::Present => "present",
        HidrawNodeState::Missing => "missing",
        HidrawNodeState::MetadataUnavailable => "metadata_unavailable",
    }
}
