use crate::model::{
    CapabilityProfile, CommandKind, CompoundFailure, CompoundStageRecord, CompoundStageStatus,
    CompoundStatus, DeviceKind, DeviceStatus, DeviceSummary, DmiIdentity, DoctorReport,
    DryRunReport, EffectiveAccess, ExecutionMode, Finding, FindingLevel, HidrawNodeState,
    InfoReport, KeyboardStatusReport, ListReport, LiveExecutedStep, LiveWriteReport,
    PermissionEstimate, PowerProfileReport, PowerStateSummary, SetAllPlanStage, SetAllReport,
    ValidationState, ZoneSummary, ZonesReport,
};
use crate::presentation::{
    doctor_human, dry_run_human, info_human, keyboard_status_human, list_human, live_write_human,
    power_profile_human, to_json, zones_human,
};
use crate::profile::{DescriptorEvidence, CONFIRMED_BIOS_VERSION};
use crate::protocol::{api_v4, api_v5, power_v4, LogicalColor, Rgb};
use crate::resume_state::{SetAllStateStore, SystemSetAllStateStore};
use crate::sysfs;
use crate::targets::{
    chassis_targets, expand_chassis_targets, expand_keyboard_targets, keyboard_targets,
    lookup_chassis_target, lookup_keyboard_target, power_profile_validation, TargetDefinition,
    TargetError,
};
use crate::transport::{
    execute_aw_elc, execute_keyboard, execute_keyboard_status, execute_power_profile,
    execute_set_all, prepare_aw_elc, prepare_keyboard, prepare_power_profile, prepare_set_all,
    ExecutionResult, HidApiKeyboardFactory, RusbAwElcFactory, SetAllExecutionOutcome,
    SetAllLiveIntent, SystemDiscovery,
};
use serde::Serialize;
use std::error::Error;
use std::fmt;
use std::io;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequestedDevice {
    Keyboard,
    Chassis,
}

impl RequestedDevice {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Keyboard => "keyboard",
            Self::Chassis => "chassis",
        }
    }

    fn kind(self) -> DeviceKind {
        match self {
            Self::Keyboard => DeviceKind::Keyboard,
            Self::Chassis => DeviceKind::Chassis,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CliCommand {
    List,
    Info,
    Doctor,
    Zones {
        device: Option<RequestedDevice>,
    },
    KeyboardStatus,
    PowerProfile {
        color: Rgb,
        dry_run: bool,
        apply: bool,
        experimental: bool,
        confirm_power_profile_write: bool,
    },
    SetAll {
        color: Rgb,
        dry_run: bool,
        apply: bool,
        experimental: bool,
        confirm_live_write: bool,
        confirm_power_profile_write: bool,
        confirm_set_all_write: bool,
    },
    Set {
        device: RequestedDevice,
        targets: Vec<String>,
        color: Rgb,
        dry_run: bool,
        apply: bool,
        experimental: bool,
        confirm_live_write: bool,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Cli {
    pub command: CliCommand,
    pub json: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ChassisLiveIntent {
    pub(crate) target: &'static str,
    pub(crate) logical_id: u8,
    pub(crate) color: Rgb,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct KeyboardLiveTarget {
    pub(crate) target: &'static str,
    pub(crate) logical_id: u8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct KeyboardLiveIntent {
    pub(crate) targets: Vec<KeyboardLiveTarget>,
    pub(crate) color: Rgb,
}

pub(crate) struct LiveExecutionReceipt {
    pub(crate) steps: Vec<LiveExecutedStep>,
}

pub(crate) trait LiveKeyboardExecutor {
    fn execute(&mut self, intent: KeyboardLiveIntent) -> Result<LiveExecutionReceipt, CliRunError>;
}

pub(crate) struct KeyboardStatusReceipt {
    pub(crate) query_write_length: usize,
    pub(crate) response: Vec<u8>,
}

pub(crate) trait KeyboardStatusExecutor {
    fn execute(&mut self) -> Result<KeyboardStatusReceipt, CliRunError>;
}

pub(crate) trait LiveChassisExecutor {
    fn execute(&mut self, intent: ChassisLiveIntent) -> Result<LiveExecutionReceipt, CliRunError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PowerProfileLiveIntent {
    pub(crate) color: Rgb,
}

pub(crate) trait LiveSetAllExecutor {
    fn execute(&mut self, intent: SetAllLiveIntent) -> Result<SetAllExecutionOutcome, CliRunError>;
}

pub(crate) trait LivePowerProfileExecutor {
    fn execute(
        &mut self,
        intent: PowerProfileLiveIntent,
    ) -> Result<LiveExecutionReceipt, CliRunError>;
}

pub(crate) trait WarningSink {
    fn warn(&mut self, message: &str);
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CliRunError {
    pub code: &'static str,
    pub message: String,
    pub transport_performed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report: Option<Box<SetAllReport>>,
}

impl CliRunError {
    fn validation(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            transport_performed: false,
            report: None,
        }
    }

    pub(crate) fn transport(message: impl Into<String>) -> Self {
        Self::validation("transport_failed", message)
    }
}

impl fmt::Display for CliRunError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for CliRunError {}

struct SystemLiveKeyboardExecutor;

impl LiveKeyboardExecutor for SystemLiveKeyboardExecutor {
    fn execute(&mut self, intent: KeyboardLiveIntent) -> Result<LiveExecutionReceipt, CliRunError> {
        let assignments = intent
            .targets
            .iter()
            .map(|target| LogicalColor::new(target.logical_id, intent.color))
            .collect::<Vec<_>>();
        let prepared = prepare_keyboard(assignments)
            .map_err(|error| CliRunError::transport(error.to_string()))?;
        let result = execute_keyboard(prepared, &mut SystemDiscovery, &mut HidApiKeyboardFactory)
            .map_err(|error| CliRunError::transport(error.to_string()))?;
        execution_receipt(result)
    }
}

struct SystemKeyboardStatusExecutor;

impl KeyboardStatusExecutor for SystemKeyboardStatusExecutor {
    fn execute(&mut self) -> Result<KeyboardStatusReceipt, CliRunError> {
        let capture = execute_keyboard_status(&mut SystemDiscovery, &mut HidApiKeyboardFactory)
            .map_err(|error| CliRunError::transport(error.to_string()))?;
        Ok(KeyboardStatusReceipt {
            query_write_length: capture.query_write_length,
            response: capture.response,
        })
    }
}

struct SystemLiveSetAllExecutor;

impl LiveSetAllExecutor for SystemLiveSetAllExecutor {
    fn execute(&mut self, intent: SetAllLiveIntent) -> Result<SetAllExecutionOutcome, CliRunError> {
        let prepared =
            prepare_set_all(intent).map_err(|error| CliRunError::transport(error.to_string()))?;
        execute_set_all(
            prepared,
            &mut SystemDiscovery,
            &mut HidApiKeyboardFactory,
            &mut RusbAwElcFactory,
        )
        .map_err(|error| CliRunError::transport(error.to_string()))
    }
}

struct SystemLivePowerProfileExecutor;

impl LivePowerProfileExecutor for SystemLivePowerProfileExecutor {
    fn execute(
        &mut self,
        intent: PowerProfileLiveIntent,
    ) -> Result<LiveExecutionReceipt, CliRunError> {
        let prepared = prepare_power_profile(intent.color);
        let result = execute_power_profile(prepared, &mut SystemDiscovery, &mut RusbAwElcFactory)
            .map_err(|error| CliRunError::transport(error.to_string()))?;
        execution_receipt(result)
    }
}

struct SystemLiveChassisExecutor;

impl LiveChassisExecutor for SystemLiveChassisExecutor {
    fn execute(&mut self, intent: ChassisLiveIntent) -> Result<LiveExecutionReceipt, CliRunError> {
        let prepared = prepare_aw_elc(vec![LogicalColor::new(intent.logical_id, intent.color)])
            .map_err(|error| CliRunError::transport(error.to_string()))?;
        let result = execute_aw_elc(prepared, &mut SystemDiscovery, &mut RusbAwElcFactory)
            .map_err(|error| CliRunError::transport(error.to_string()))?;
        execution_receipt(result)
    }
}

fn execution_receipt(result: ExecutionResult) -> Result<LiveExecutionReceipt, CliRunError> {
    if !result.completed() {
        return Err(CliRunError::transport(
            "transport returned an incomplete execution",
        ));
    }
    Ok(LiveExecutionReceipt {
        steps: result
            .steps()
            .iter()
            .map(|step| LiveExecutedStep {
                name: step.name(),
                transferred_length: step.actual_length(),
            })
            .collect(),
    })
}

struct StderrWarnings;

impl WarningSink for StderrWarnings {
    fn warn(&mut self, message: &str) {
        eprintln!("{message}");
    }
}

pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Cli, String> {
    let args = args.into_iter().collect::<Vec<_>>();
    let Some(command) = args.first().map(String::as_str) else {
        return Err(usage().into());
    };
    match command {
        "list" => parse_simple(CliCommand::List, &args[1..]),
        "info" => parse_simple(CliCommand::Info, &args[1..]),
        "doctor" => parse_simple(CliCommand::Doctor, &args[1..]),
        "zones" => parse_zones(&args[1..]),
        "keyboard-status" => parse_keyboard_status(&args[1..]),
        "power-profile" => parse_power_profile(&args[1..]),
        "set" => parse_set(&args[1..]),
        "set-all" => parse_set_all(&args[1..]),
        "help" | "--help" | "-h" => Err(usage().into()),
        other => Err(format!("unknown command '{other}'\n\n{}", usage())),
    }
}

pub fn run(cli: Cli) -> Result<String, Box<dyn Error>> {
    match cli.command {
        CliCommand::Zones { .. } => run_with_inventory(cli, empty_dmi(), Vec::new()),
        CliCommand::KeyboardStatus => run_keyboard_status_with_services(
            cli.json,
            &mut SystemKeyboardStatusExecutor,
            &mut StderrWarnings,
        )
        .map_err(|error| Box::new(error) as Box<dyn Error>),
        CliCommand::SetAll { .. } => {
            let json = cli.json;
            let mut state_store = SystemSetAllStateStore;
            if json {
                run_set_all_with_services_and_state(
                    cli,
                    &mut SystemLiveSetAllExecutor,
                    &mut DiscardWarnings,
                    &mut state_store,
                )
            } else {
                run_set_all_with_services_and_state(
                    cli,
                    &mut SystemLiveSetAllExecutor,
                    &mut StderrWarnings,
                    &mut state_store,
                )
            }
            .map_err(|error| Box::new(error) as Box<dyn Error>)
        }
        CliCommand::PowerProfile { .. } => {
            let inventory = sysfs::discover_for_planning()?;
            run_power_profile_with_services(
                cli,
                inventory,
                &mut SystemLivePowerProfileExecutor,
                &mut StderrWarnings,
            )
            .map_err(|error| Box::new(error) as Box<dyn Error>)
        }
        CliCommand::Set { .. } => {
            let inventory = sysfs::discover_for_planning()?;
            run_with_all_services(
                cli,
                inventory,
                &mut SystemLiveChassisExecutor,
                &mut SystemLiveKeyboardExecutor,
                &mut StderrWarnings,
            )
            .map_err(|error| Box::new(error) as Box<dyn Error>)
        }
        _ => {
            let (dmi, devices) = sysfs::discover()?;
            run_with_inventory(cli, dmi, devices)
        }
    }
}

pub fn run_with_inventory(
    cli: Cli,
    dmi: DmiIdentity,
    found: Vec<DeviceSummary>,
) -> Result<String, Box<dyn Error>> {
    match cli.command {
        CliCommand::List => {
            let report = ListReport {
                schema_version: 1,
                command: CommandKind::List,
                supported_system: dmi.supported,
                devices: found,
            };
            render(cli.json, &report, || list_human(&report))
        }
        CliCommand::Info => {
            let devices = with_missing(found);
            let report = InfoReport {
                schema_version: 1,
                command: CommandKind::Info,
                dmi,
                capabilities: capabilities(),
                devices,
                safety: "read_only_no_hardware_reports",
            };
            render(cli.json, &report, || info_human(&report))
        }
        CliCommand::Doctor => {
            let devices = with_missing(found);
            let report = doctor_report(&dmi, &devices);
            render(cli.json, &report, || doctor_human(&report))
        }
        CliCommand::Zones { device } => {
            let report = zones_report(device);
            render(cli.json, &report, || zones_human(&report))
        }
        CliCommand::KeyboardStatus => Err(cli_error(
            "keyboard-status requires the sealed live status executor",
        )),
        CliCommand::SetAll { apply: true, .. } => Err(cli_error(
            "live set-all apply requires the sealed runtime executor; injected inventory is dry-run only",
        )),
        CliCommand::SetAll { .. } => {
            let mut executor = DisabledLiveSetAllExecutor;
            let mut warnings = DiscardWarnings;
            run_set_all_with_services_and_state(
                cli,
                &mut executor,
                &mut warnings,
                &mut DiscardSetAllStateStore,
            )
            .map_err(|error| Box::new(error) as Box<dyn Error>)
        },
        CliCommand::PowerProfile { apply: true, .. } => Err(cli_error(
            "live power-profile apply requires the sealed runtime executor; injected inventory is dry-run only",
        )),
        CliCommand::PowerProfile { color, .. } => {
            validate_power_profile_gate(&dmi, &found)?;
            let report = power_profile_report(
                ExecutionMode::DryRun,
                false,
                color,
                Vec::new(),
            );
            render(cli.json, &report, || power_profile_human(&report))
        }
        CliCommand::Set { apply: true, .. } => Err(cli_error(
            "live apply requires the sealed runtime executor; injected inventory is dry-run only",
        )),
        CliCommand::Set { .. } => {
            let mut chassis_executor = DisabledLiveChassisExecutor;
            let mut keyboard_executor = DisabledLiveKeyboardExecutor;
            let mut warnings = DiscardWarnings;
            run_with_all_services(
                cli,
                (dmi, found),
                &mut chassis_executor,
                &mut keyboard_executor,
                &mut warnings,
            )
            .map_err(|error| Box::new(error) as Box<dyn Error>)
        }
    }
}

struct DisabledLiveSetAllExecutor;

impl LiveSetAllExecutor for DisabledLiveSetAllExecutor {
    fn execute(
        &mut self,
        _intent: SetAllLiveIntent,
    ) -> Result<SetAllExecutionOutcome, CliRunError> {
        Err(CliRunError::validation(
            "live_executor_unavailable",
            "live set-all execution is unavailable in the injected-inventory path",
        ))
    }
}

struct DisabledLiveKeyboardExecutor;

impl LiveKeyboardExecutor for DisabledLiveKeyboardExecutor {
    fn execute(
        &mut self,
        _intent: KeyboardLiveIntent,
    ) -> Result<LiveExecutionReceipt, CliRunError> {
        Err(CliRunError::validation(
            "live_executor_unavailable",
            "live execution is unavailable in the injected-inventory path",
        ))
    }
}

struct DisabledLiveChassisExecutor;

impl LiveChassisExecutor for DisabledLiveChassisExecutor {
    fn execute(&mut self, _intent: ChassisLiveIntent) -> Result<LiveExecutionReceipt, CliRunError> {
        Err(CliRunError::validation(
            "live_executor_unavailable",
            "live execution is unavailable in the injected-inventory path",
        ))
    }
}

struct DiscardWarnings;

impl WarningSink for DiscardWarnings {
    fn warn(&mut self, _message: &str) {}
}

struct DiscardSetAllStateStore;

impl SetAllStateStore for DiscardSetAllStateStore {
    fn persist_color(&mut self, _color: Rgb) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
pub(crate) fn run_with_services(
    cli: Cli,
    inventory: (DmiIdentity, Vec<DeviceSummary>),
    chassis_executor: &mut impl LiveChassisExecutor,
    warnings: &mut impl WarningSink,
) -> Result<String, CliRunError> {
    run_with_all_services(
        cli,
        inventory,
        chassis_executor,
        &mut DisabledLiveKeyboardExecutor,
        warnings,
    )
}

pub(crate) fn run_keyboard_status_with_services(
    json: bool,
    executor: &mut impl KeyboardStatusExecutor,
    warnings: &mut impl WarningSink,
) -> Result<String, CliRunError> {
    if !json {
        warnings.warn("WARNING: experimental diagnostic keyboard status capture performs exactly one 64-byte HID feature status-query write and one feature read. It sends no reset, color_set, loop, or update frame and does not intentionally change any RGB assignment.");
    }
    let receipt = executor.execute()?;
    let response_hex = receipt
        .response
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let report = KeyboardStatusReport {
        schema_version: 1,
        command: CommandKind::KeyboardStatus,
        transport_performed: true,
        color_frames_sent: false,
        query_write_length: receipt.query_write_length,
        response_length: receipt.response.len(),
        response_hex,
        persistence: "not_claimed",
        color_readback: "not_performed_status_bytes_only",
    };
    render(json, &report, || keyboard_status_human(&report))
        .map_err(|error| CliRunError::validation("render_failed", error.to_string()))
}

pub(crate) fn run_power_profile_with_services(
    cli: Cli,
    inventory: (DmiIdentity, Vec<DeviceSummary>),
    executor: &mut impl LivePowerProfileExecutor,
    warnings: &mut impl WarningSink,
) -> Result<String, CliRunError> {
    let CliCommand::PowerProfile {
        color,
        dry_run,
        apply,
        experimental: _,
        confirm_power_profile_write: _,
    } = cli.command
    else {
        return Err(CliRunError::validation(
            "invalid_service_command",
            "the power-profile service boundary accepts only power-profile commands",
        ));
    };
    let (dmi, found) = inventory;
    validate_power_profile_gate(&dmi, &found)
        .map_err(|error| CliRunError::validation("hardware_gate_failed", error.to_string()))?;

    if dry_run {
        let report = power_profile_report(ExecutionMode::DryRun, false, color, Vec::new());
        return render(cli.json, &report, || power_profile_human(&report))
            .map_err(|error| CliRunError::validation("render_failed", error.to_string()));
    }
    if !apply {
        return Err(CliRunError::validation(
            "execution_mode_required",
            "power-profile requires exactly one of --dry-run or --apply",
        ));
    }
    if !cli.json {
        warnings.warn(&format!(
            "WARNING: special six-state profile write; power ID4; 34 ordered 33-byte writes; all AC/battery states set to one color {}; ordinary timeout exposure up to about 17 seconds (34×500ms) plus scheduling/acquisition; any later write failure can leave a partial profile; unknown persistence; no readback/restore; no retry; no status polling.",
            color.hex()
        ));
    }
    let receipt = executor.execute(PowerProfileLiveIntent { color })?;
    let canonical = power_v4::encode_equal_color(color);
    if receipt.steps.len() != canonical.steps().len()
        || receipt
            .steps
            .iter()
            .zip(canonical.steps())
            .any(|(actual, expected)| {
                actual.name != expected.name() || actual.transferred_length != 33
            })
    {
        return Err(CliRunError::validation(
            "incomplete_power_profile_execution",
            "power-profile executor returned a non-canonical completion receipt",
        ));
    }
    let report = power_profile_report(ExecutionMode::Apply, true, color, receipt.steps);
    render(cli.json, &report, || power_profile_human(&report))
        .map_err(|error| CliRunError::validation("render_failed", error.to_string()))
}

fn power_profile_report(
    mode: ExecutionMode,
    transport_performed: bool,
    color: Rgb,
    executed_steps: Vec<LiveExecutedStep>,
) -> PowerProfileReport {
    let plan = power_v4::encode_equal_color(color);
    PowerProfileReport {
        schema_version: 1,
        command: CommandKind::PowerProfile,
        mode,
        profile_kind: "power_state_profile",
        power_profile: true,
        transport_performed,
        controller: "187c:0551",
        power_logical_id: 4,
        validation: power_profile_validation(color),
        color: color.hex(),
        color_applies_to: ["ac", "dc"],
        states: power_v4::STATES
            .iter()
            .map(|state| PowerStateSummary {
                id: state.id,
                name: state.name,
                packet_count: state.packet_count,
            })
            .collect(),
        packet_count: plan.steps().len(),
        plan,
        executed_steps,
        persistence: "unknown_power_profile_state",
        state_restore_available: false,
        readback_available: false,
    }
}

#[cfg(test)]
pub(crate) fn run_set_all_with_services(
    cli: Cli,
    executor: &mut impl LiveSetAllExecutor,
    warnings: &mut impl WarningSink,
) -> Result<String, CliRunError> {
    run_set_all_with_services_and_state(cli, executor, warnings, &mut DiscardSetAllStateStore)
}

pub(crate) fn run_set_all_with_services_and_state(
    cli: Cli,
    executor: &mut impl LiveSetAllExecutor,
    warnings: &mut impl WarningSink,
    state_store: &mut impl SetAllStateStore,
) -> Result<String, CliRunError> {
    let CliCommand::SetAll {
        color,
        dry_run,
        apply,
        ..
    } = cli.command
    else {
        return Err(CliRunError::validation(
            "invalid_service_command",
            "the set-all service boundary accepts only set-all commands",
        ));
    };
    let mut report = set_all_report(
        color,
        if dry_run {
            ExecutionMode::DryRun
        } else {
            ExecutionMode::Apply
        },
    );
    if dry_run {
        return render(cli.json, &report, || {
            crate::presentation::set_all_human(&report)
        })
        .map_err(|error| CliRunError::validation("render_failed", error.to_string()));
    }
    if !apply {
        return Err(CliRunError::validation(
            "execution_mode_required",
            "set-all requires exactly one of --dry-run or --apply",
        ));
    }
    warnings.warn(&format!(
        "WARNING: experimental compound write spans two controllers and four RGB groups in fixed order keyboard -> aw_static_touchpad_back -> power_profile; exactly 49 transport operations; ordinary timeout envelope about 74 seconds plus scheduling/acquisition; non-atomic partial-state risk; power six-state persistence unknown; no rollback/readback/restore/retry. color={}",
        color.hex()
    ));
    let outcome = match executor.execute(SetAllLiveIntent { color }) {
        Ok(outcome) => outcome,
        Err(error) => {
            report.overall_status = Some(CompoundStatus::PreflightFailure);
            report.stages = preflight_stage_records(&error);
            let message = format!("set-all preflight failed: {}", error.message);
            return Err(CliRunError {
                code: "set_all_preflight_failure",
                message,
                transport_performed: false,
                report: Some(Box::new(report)),
            });
        }
    };
    let completed_canonical_profile = set_all_completed_canonically(&outcome);
    report.transport_attempted = outcome.transport_attempted();
    report.transport_performed = outcome.transport_performed();
    report.overall_status = Some(if completed_canonical_profile {
        CompoundStatus::Completed
    } else {
        CompoundStatus::PartialFailure
    });
    report.stages = outcome
        .stages
        .iter()
        .enumerate()
        .map(|(index, stage)| {
            let failed_before = outcome.stages[..index]
                .iter()
                .any(|prior| prior.failure.is_some());
            CompoundStageRecord {
                stage: stage.name,
                status: if stage.failure.is_some() {
                    CompoundStageStatus::Failed
                } else if failed_before {
                    CompoundStageStatus::NotStarted
                } else {
                    CompoundStageStatus::Completed
                },
                expected_steps: stage.expected_steps,
                completed_steps: stage.completed_steps,
                transport_attempted: stage.transport_attempted,
                failure: stage.failure.as_ref().map(|failure| CompoundFailure {
                    code: failure.code_text(),
                    step: failure.step().map(str::to_string),
                    message: failure.message().to_string(),
                }),
            }
        })
        .collect();
    if !completed_canonical_profile {
        if let Some(failed) = report
            .stages
            .iter()
            .find(|stage| stage.status == CompoundStageStatus::Failed)
        {
            let failure = failed.failure.as_ref().expect("failed stage has failure");
            let message = format!(
                "set-all failed at {} after {}/{} steps: {}",
                failed.stage, failed.completed_steps, failed.expected_steps, failure.message
            );
            return Err(CliRunError {
                code: "set_all_partial_failure",
                message,
                transport_performed: report.transport_performed,
                report: Some(Box::new(report)),
            });
        }
        return Err(CliRunError {
            code: "incomplete_set_all_execution",
            message: "set-all executor returned a non-canonical completion receipt; state was not persisted".into(),
            transport_performed: report.transport_performed,
            report: Some(Box::new(report)),
        });
    }
    state_store.persist_color(color).map_err(|error| CliRunError {
        code: "resume_state_persist_failed",
        message: format!("set-all completed all 49 operations, but failed to persist the last successful set-all state: {error}"),
        transport_performed: true,
        report: None,
    })?;
    render(cli.json, &report, || {
        crate::presentation::set_all_human(&report)
    })
    .map_err(|error| CliRunError::validation("render_failed", error.to_string()))
}

fn set_all_completed_canonically(outcome: &SetAllExecutionOutcome) -> bool {
    const EXPECTED: [(&str, usize); 3] = [
        ("keyboard", 11),
        ("aw_static_touchpad_back", 4),
        ("power_profile", 34),
    ];
    outcome.completed()
        && outcome.stages.len() == EXPECTED.len()
        && outcome
            .stages
            .iter()
            .zip(EXPECTED)
            .all(|(stage, (name, steps))| {
                stage.name == name
                    && stage.expected_steps == steps
                    && stage.completed_steps == steps
                    && stage.transport_attempted
            })
}

fn preflight_stage_records(error: &CliRunError) -> Vec<CompoundStageRecord> {
    [
        ("keyboard", 11),
        ("aw_static_touchpad_back", 4),
        ("power_profile", 34),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (stage, expected_steps))| CompoundStageRecord {
        stage,
        status: if index == 0 {
            CompoundStageStatus::Failed
        } else {
            CompoundStageStatus::NotStarted
        },
        expected_steps,
        completed_steps: 0,
        transport_attempted: false,
        failure: (index == 0).then(|| CompoundFailure {
            code: error.code.to_string(),
            step: None,
            message: error.message.clone(),
        }),
    })
    .collect()
}

const HOT_PINK_SET_ALL_VALIDATION_EVIDENCE: &str = "Exact one-run global set-all #ff69b4 validation on Alienware m16 R2 BIOS 1.21.0 with keyboard 0d62:d2b1 and AW-ELC 187c:0551: keyboard 11/11, combined touchpad/back 4/4, and power profile 34/34 (49/49 total), no retry; user visibly confirmed keyboard, touchpad, rear, and power Hot Pink. Immediate read-only sysfs: AC online=1, BAT0 Charging, capacity=36%; power observation covers only current AC-charging state. Non-atomic; no rollback/readback/restore; power persistence unknown. Other colors, power states, and individual keyboard ID identity beyond existing records remain unvalidated.";

fn set_all_validation(color: Rgb) -> (ValidationState, String) {
    if color == Rgb::new(0xff, 0x69, 0xb4) {
        (
            ValidationState::ExactSetAllHotPinkLiveValidated,
            HOT_PINK_SET_ALL_VALIDATION_EVIDENCE.into(),
        )
    } else {
        (
            ValidationState::MappingDerivedUnvalidated,
            format!(
                "No exact global set-all live validation exists for {}; only the exact #ff69b4 canonical command/color has global compound evidence.",
                color.hex()
            ),
        )
    }
}

fn set_all_report(color: Rgb, mode: ExecutionMode) -> SetAllReport {
    let keyboard_assignments = keyboard_targets()
        .iter()
        .map(|target| LogicalColor::new(target.logical_id, color))
        .collect::<Vec<_>>();
    let keyboard_plan =
        api_v5::encode_static(&keyboard_assignments).expect("canonical keyboard catalog");
    let aw_plan =
        api_v4::encode_static(&[LogicalColor::new(0, color), LogicalColor::new(2, color)])
            .expect("canonical AW static IDs");
    let power_plan = power_v4::encode_equal_color(color);
    let (validation, validation_evidence) = set_all_validation(color);
    SetAllReport {
        schema_version: 1,
        command: CommandKind::SetAll,
        mode,
        color: color.hex(),
        validation,
        validation_evidence,
        transport_attempted: false,
        transport_performed: false,
        keyboard: SetAllPlanStage {
            assignment_count: 85,
            color_frame_count: Some(6),
            logical_ids: Vec::new(),
            state_count: None,
            step_count: 11,
            plan: keyboard_plan,
        },
        aw_static_touchpad_back: SetAllPlanStage {
            assignment_count: 2,
            color_frame_count: None,
            logical_ids: vec![0, 2],
            state_count: None,
            step_count: 4,
            plan: aw_plan,
        },
        power_profile: SetAllPlanStage {
            assignment_count: 1,
            color_frame_count: None,
            logical_ids: vec![4],
            state_count: Some(6),
            step_count: 34,
            plan: power_plan,
        },
        total_transport_steps: 49,
        safety_warning: "Two controllers/four RGB groups; fixed order keyboard -> aw_static_touchpad_back -> power_profile; 49 operations; ordinary timeout envelope about 74 seconds plus scheduling/acquisition; non-atomic partial-state risk; power six-state persistence unknown; no rollback/readback/restore/retry.",
        overall_status: None,
        stages: Vec::new(),
        rollback: "not_attempted_not_available",
        persistence: "unknown_power_profile_state_not_claimed_elsewhere",
        readback: "not_performed",
    }
}

pub(crate) fn run_with_all_services(
    cli: Cli,
    inventory: (DmiIdentity, Vec<DeviceSummary>),
    chassis_executor: &mut impl LiveChassisExecutor,
    keyboard_executor: &mut impl LiveKeyboardExecutor,
    warnings: &mut impl WarningSink,
) -> Result<String, CliRunError> {
    let CliCommand::Set {
        device,
        targets,
        color,
        dry_run,
        apply,
        experimental: _,
        confirm_live_write: _,
    } = cli.command
    else {
        return Err(CliRunError::validation(
            "invalid_service_command",
            "the live service boundary accepts only set commands",
        ));
    };
    let (dmi, found) = inventory;

    if dry_run {
        let resolved = resolve_targets(device, &targets)
            .map_err(|error| CliRunError::validation("invalid_target", error.to_string()))?;
        validate_plan_gate(device, &dmi, &found)
            .map_err(|error| CliRunError::validation("hardware_gate_failed", error.to_string()))?;
        let assignments = resolved
            .iter()
            .map(|target| LogicalColor::new(target.logical_id, color))
            .collect::<Vec<_>>();
        let plan = match device {
            RequestedDevice::Keyboard => api_v5::encode_static(&assignments),
            RequestedDevice::Chassis => api_v4::encode_static(&assignments),
        }
        .map_err(|error| CliRunError::validation("invalid_plan", error.to_string()))?;
        let report = DryRunReport {
            schema_version: 1,
            mode: ExecutionMode::DryRun,
            transport_performed: false,
            requested_device: device.kind(),
            plan,
        };
        return render(cli.json, &report, || dry_run_human(&report))
            .map_err(|error| CliRunError::validation("render_failed", error.to_string()));
    }

    if !apply {
        return Err(CliRunError::validation(
            "execution_mode_required",
            "set requires exactly one of --dry-run or --apply",
        ));
    }
    match device {
        RequestedDevice::Keyboard => validate_keyboard_live_target_names(&targets)
            .map_err(|error| CliRunError::validation("invalid_keyboard_targets", error))?,
        RequestedDevice::Chassis => {
            if targets.len() != 1 || targets[0].eq_ignore_ascii_case("all") {
                return Err(CliRunError::validation(
                    "single_known_target_required",
                    "live chassis apply requires exactly one named target; 'all', lists, and duplicates are rejected",
                ));
            }
        }
    }
    let requested_target = targets.join(",");
    let resolved = resolve_targets(device, &targets)
        .map_err(|error| CliRunError::validation("invalid_target", error.to_string()))?;
    if resolved.is_empty()
        || (device == RequestedDevice::Keyboard && resolved.len() > keyboard_targets().len())
    {
        return Err(CliRunError::validation(
            "keyboard_target_limit_exceeded",
            format!(
                "keyboard live apply requires 1..={} unique keys",
                keyboard_targets().len()
            ),
        ));
    }
    if device == RequestedDevice::Chassis && resolved.len() != 1 {
        return Err(CliRunError::validation(
            "single_known_target_required",
            "live chassis apply requires exactly one canonical target",
        ));
    }
    validate_plan_gate(device, &dmi, &found)
        .map_err(|error| CliRunError::validation("hardware_gate_failed", error.to_string()))?;
    let canonical_targets = resolved
        .iter()
        .map(|target| target.name.to_string())
        .collect::<Vec<_>>();
    let resolved_target = canonical_targets.join(",");
    let target_count = resolved.len();
    let set_color_frames = if device == RequestedDevice::Keyboard {
        target_count.div_ceil(15)
    } else {
        1
    };

    let (receipt, controller_vid, controller_pid) = match device {
        RequestedDevice::Keyboard => {
            let intent = KeyboardLiveIntent {
                targets: resolved
                    .iter()
                    .map(|target| KeyboardLiveTarget {
                        target: target.name,
                        logical_id: target.logical_id,
                    })
                    .collect(),
                color,
            };
            if !cli.json {
                let one_key_compatibility = if target_count == 1 {
                    format!("key={resolved_target}; ")
                } else {
                    String::new()
                };
                let target_summary = if requested_target.eq_ignore_ascii_case("all") {
                    format!("all known keyboard targets; count={target_count}")
                } else {
                    format!(
                        "requested_keys={requested_target} canonical_keys={resolved_target} count={target_count}"
                    )
                };
                let operations = 5 + set_color_frames;
                let timeout_seconds = operations * 5;
                warnings.warn(&format!(
                    "WARNING: experimental keyboard feature write; {one_key_compatibility}{target_summary} color={}; set_color_frames={set_color_frames}; kernel-bounded feature ioctl; HID operations={operations}; ordinary timeout estimate up to about {timeout_seconds} seconds plus scheduling and cancellation delay. A failure after an earlier color frame can leave a partial applied state; no reliable color readback, automatic restore, or persistence claim.",
                    intent.color.hex()
                ));
            }
            (keyboard_executor.execute(intent)?, "0d62", "d2b1")
        }
        RequestedDevice::Chassis => {
            let target = resolved[0];
            let intent = ChassisLiveIntent {
                target: target.name,
                logical_id: target.logical_id,
                color,
            };
            if !cli.json {
                warnings.warn(&format!(
                    "WARNING: experimental AW-ELC only; target={} color={}; volatile static write; no reliable color readback and no automatic state restore.",
                    intent.target,
                    intent.color.hex()
                ));
            }
            (chassis_executor.execute(intent)?, "187c", "0551")
        }
    };
    let report = LiveWriteReport {
        schema_version: 1,
        command: CommandKind::Set,
        mode: ExecutionMode::Apply,
        transport_performed: true,
        requested_device: device.kind(),
        requested_target,
        resolved_target,
        canonical_targets,
        target_count,
        set_color_frames,
        color: color.hex(),
        controller_vid,
        controller_pid,
        executed_steps: receipt.steps,
        state_restore_available: false,
        persistence: "not_claimed",
    };
    render(cli.json, &report, || live_write_human(&report))
        .map_err(|error| CliRunError::validation("render_failed", error.to_string()))
}

pub fn usage() -> &'static str {
    "Usage:\n  alienrgb list [--json]\n  alienrgb info [--json]\n  alienrgb doctor [--json]\n  alienrgb zones [--device keyboard|chassis] [--json]\n  alienrgb keyboard-status --experimental --confirm-live-query [--json]\n  alienrgb power-profile --color <RRGGBB|#RRGGBB> --dry-run [--json]\n  alienrgb power-profile --color <RRGGBB> --apply --experimental --confirm-power-profile-write [--json]\n  alienrgb set-all --color <RRGGBB|#RRGGBB> --dry-run [--json]\n  alienrgb set-all --color <RRGGBB> --apply --experimental --confirm-live-write --confirm-power-profile-write --confirm-set-all-write [--json]\n  alienrgb set --device <keyboard|chassis> --target <name>[,<name>...] --color <RRGGBB|#RRGGBB> --dry-run [--json]\n  alienrgb set --device chassis --target <one-known-zone> --color <RRGGBB|#RRGGBB> --apply --experimental --confirm-live-write [--json]\n  alienrgb set --device keyboard --target <1..=75-comma-separated-nonnumeric-key-names|all> --color <RRGGBB|#RRGGBB> --apply --experimental --confirm-live-write [--json]"
}

fn parse_set_all(args: &[String]) -> Result<Cli, String> {
    let mut color = None;
    let mut color_has_hash_prefix = false;
    let mut dry_run = false;
    let mut apply = false;
    let mut experimental = false;
    let mut confirm_live_write = false;
    let mut confirm_power_profile_write = false;
    let mut confirm_set_all_write = false;
    let mut json = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--color" => {
                if color.is_some() {
                    return Err("--color may be provided only once".into());
                }
                let value = required_value(args, &mut index, "--color")?;
                color_has_hash_prefix = value.starts_with('#');
                color = Some(parse_color(value)?);
            }
            "--dry-run" if !dry_run => dry_run = true,
            "--apply" if !apply => apply = true,
            "--experimental" if !experimental => experimental = true,
            "--confirm-live-write" if !confirm_live_write => confirm_live_write = true,
            "--confirm-power-profile-write" if !confirm_power_profile_write => {
                confirm_power_profile_write = true
            }
            "--confirm-set-all-write" if !confirm_set_all_write => confirm_set_all_write = true,
            "--json" if !json => json = true,
            "--help" | "-h" => return Err(usage().into()),
            argument => {
                return Err(format!(
                    "unexpected or duplicate argument '{argument}'\n\n{}",
                    usage()
                ))
            }
        }
        index += 1;
    }
    let color = color.ok_or("set-all requires --color <RRGGBB|#RRGGBB>")?;
    if dry_run && apply {
        return Err("--dry-run and --apply are mutually exclusive".into());
    }
    if !dry_run && !apply {
        return Err("set-all requires exactly one of --dry-run or --apply".into());
    }
    if dry_run
        && (experimental
            || confirm_live_write
            || confirm_power_profile_write
            || confirm_set_all_write)
    {
        return Err("set-all confirmation flags require --apply".into());
    }
    if apply && color_has_hash_prefix {
        return Err("set-all --apply requires color as RRGGBB without a leading '#'".into());
    }
    if apply && !experimental {
        return Err("set-all --apply requires --experimental".into());
    }
    if apply && !confirm_live_write {
        return Err("set-all --apply requires --confirm-live-write".into());
    }
    if apply && !confirm_power_profile_write {
        return Err("set-all --apply requires --confirm-power-profile-write".into());
    }
    if apply && !confirm_set_all_write {
        return Err("set-all --apply requires --confirm-set-all-write".into());
    }
    Ok(Cli {
        command: CliCommand::SetAll {
            color,
            dry_run,
            apply,
            experimental,
            confirm_live_write,
            confirm_power_profile_write,
            confirm_set_all_write,
        },
        json,
    })
}

fn parse_power_profile(args: &[String]) -> Result<Cli, String> {
    let mut color = None;
    let mut color_has_hash_prefix = false;
    let mut dry_run = false;
    let mut apply = false;
    let mut experimental = false;
    let mut confirm_power_profile_write = false;
    let mut json = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--color" => {
                if color.is_some() {
                    return Err("--color may be provided only once".into());
                }
                let value = required_value(args, &mut index, "--color")?;
                color_has_hash_prefix = value.starts_with('#');
                color = Some(parse_color(value)?);
            }
            "--dry-run" if !dry_run => dry_run = true,
            "--apply" if !apply => apply = true,
            "--experimental" if !experimental => experimental = true,
            "--confirm-power-profile-write" if !confirm_power_profile_write => {
                confirm_power_profile_write = true
            }
            "--json" if !json => json = true,
            "--help" | "-h" => return Err(usage().into()),
            argument => {
                return Err(format!(
                    "unexpected or duplicate argument '{argument}'\n\n{}",
                    usage()
                ))
            }
        }
        index += 1;
    }
    let color = color.ok_or("power-profile requires --color <RRGGBB|#RRGGBB>")?;
    if dry_run && apply {
        return Err("--dry-run and --apply are mutually exclusive".into());
    }
    if !dry_run && !apply {
        return Err("power-profile requires exactly one of --dry-run or --apply".into());
    }
    if dry_run && (experimental || confirm_power_profile_write) {
        return Err("--experimental and --confirm-power-profile-write require --apply".into());
    }
    if apply && color_has_hash_prefix {
        return Err("power-profile --apply requires color as RRGGBB without a leading '#'".into());
    }
    if apply && !experimental {
        return Err("power-profile --apply requires --experimental".into());
    }
    if apply && !confirm_power_profile_write {
        return Err("power-profile --apply requires --confirm-power-profile-write".into());
    }
    Ok(Cli {
        command: CliCommand::PowerProfile {
            color,
            dry_run,
            apply,
            experimental,
            confirm_power_profile_write,
        },
        json,
    })
}

fn parse_keyboard_status(args: &[String]) -> Result<Cli, String> {
    let mut experimental = false;
    let mut confirm = false;
    let mut json = false;
    for argument in args {
        match argument.as_str() {
            "--experimental" if !experimental => experimental = true,
            "--confirm-live-query" if !confirm => confirm = true,
            "--json" if !json => json = true,
            "--help" | "-h" => return Err(usage().into()),
            other => {
                return Err(format!(
                    "unexpected or duplicate argument '{other}'\n\n{}",
                    usage()
                ))
            }
        }
    }
    if !experimental || !confirm {
        return Err("keyboard-status requires --experimental and --confirm-live-query".into());
    }
    Ok(Cli {
        command: CliCommand::KeyboardStatus,
        json,
    })
}

fn parse_simple(command: CliCommand, args: &[String]) -> Result<Cli, String> {
    let json = parse_json_only(args)?;
    Ok(Cli { command, json })
}

fn parse_zones(args: &[String]) -> Result<Cli, String> {
    let mut device = None;
    let mut json = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--device" => {
                if device.is_some() {
                    return Err("--device may be provided only once".into());
                }
                device = Some(parse_device(required_value(args, &mut index, "--device")?)?);
            }
            "--json" if !json => json = true,
            "--help" | "-h" => return Err(usage().into()),
            argument => return Err(format!("unexpected argument '{argument}'\n\n{}", usage())),
        }
        index += 1;
    }
    Ok(Cli {
        command: CliCommand::Zones { device },
        json,
    })
}

fn parse_set(args: &[String]) -> Result<Cli, String> {
    let mut device = None;
    let mut targets = Vec::new();
    let mut target_options = 0_usize;
    let mut color = None;
    let mut dry_run = false;
    let mut apply = false;
    let mut experimental = false;
    let mut confirm_live_write = false;
    let mut json = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--device" => {
                if device.is_some() {
                    return Err("--device may be provided only once".into());
                }
                device = Some(parse_device(required_value(args, &mut index, "--device")?)?);
            }
            "--target" => {
                target_options += 1;
                let value = required_value(args, &mut index, "--target")?;
                for target in value.split(',') {
                    let target = target.trim();
                    if target.is_empty() {
                        return Err("--target contains an empty target name".into());
                    }
                    targets.push(target.to_string());
                }
            }
            "--color" => {
                if color.is_some() {
                    return Err("--color may be provided only once".into());
                }
                color = Some(parse_color(required_value(args, &mut index, "--color")?)?);
            }
            "--dry-run" if !dry_run => dry_run = true,
            "--apply" if !apply => apply = true,
            "--experimental" if !experimental => experimental = true,
            "--confirm-live-write" if !confirm_live_write => confirm_live_write = true,
            "--json" if !json => json = true,
            "--help" | "-h" => return Err(usage().into()),
            argument => return Err(format!("unexpected argument '{argument}'\n\n{}", usage())),
        }
        index += 1;
    }
    let device = device.ok_or("set requires --device <keyboard|chassis>")?;
    if targets.is_empty() {
        return Err("set requires at least one --target".into());
    }
    let color = color.ok_or("set requires --color <RRGGBB|#RRGGBB>")?;
    if dry_run && apply {
        return Err("--dry-run and --apply are mutually exclusive".into());
    }
    if !dry_run && !apply {
        return Err("set requires exactly one of --dry-run or --apply".into());
    }
    if !apply && (experimental || confirm_live_write) {
        return Err("--experimental and --confirm-live-write require --apply".into());
    }
    if apply && !experimental {
        return Err("--apply requires --experimental".into());
    }
    if apply && !confirm_live_write {
        return Err("--apply requires --confirm-live-write".into());
    }
    if device == RequestedDevice::Chassis
        && targets.iter().any(|target| {
            matches!(
                target.to_ascii_lowercase().as_str(),
                "power" | "power-button" | "all"
            )
        })
    {
        return Err("ordinary 'set --device chassis --target power' is unsupported; use 'alienrgb power-profile --color <RRGGBB|#RRGGBB> --dry-run'".into());
    }
    if apply && device == RequestedDevice::Keyboard {
        if target_options != 1 {
            return Err("keyboard live apply requires one --target comma list".into());
        }
        validate_keyboard_live_target_names(&targets)?;
    }
    Ok(Cli {
        command: CliCommand::Set {
            device,
            targets,
            color,
            dry_run,
            apply,
            experimental,
            confirm_live_write,
        },
        json,
    })
}

fn parse_json_only(args: &[String]) -> Result<bool, String> {
    match args {
        [] => Ok(false),
        [argument] if argument == "--json" => Ok(true),
        [argument] if argument == "--help" || argument == "-h" => Err(usage().into()),
        [argument, ..] => Err(format!("unexpected argument '{argument}'\n\n{}", usage())),
    }
}

fn required_value<'a>(
    args: &'a [String],
    index: &mut usize,
    option: &str,
) -> Result<&'a str, String> {
    *index += 1;
    let value = args
        .get(*index)
        .ok_or_else(|| format!("{option} requires a value"))?;
    if value.starts_with("--") {
        return Err(format!("{option} requires a value"));
    }
    Ok(value)
}

fn parse_device(value: &str) -> Result<RequestedDevice, String> {
    match value {
        "keyboard" => Ok(RequestedDevice::Keyboard),
        "chassis" => Ok(RequestedDevice::Chassis),
        _ => Err(format!(
            "unknown device '{value}'; expected 'keyboard' or 'chassis'"
        )),
    }
}

fn parse_color(value: &str) -> Result<Rgb, String> {
    let digits = value.strip_prefix('#').unwrap_or(value);
    if digits.len() != 6 || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!(
            "invalid color '{value}'; expected exactly six hexadecimal digits (RRGGBB or #RRGGBB)"
        ));
    }
    Ok(Rgb::new(
        u8::from_str_radix(&digits[0..2], 16).expect("validated hex"),
        u8::from_str_radix(&digits[2..4], 16).expect("validated hex"),
        u8::from_str_radix(&digits[4..6], 16).expect("validated hex"),
    ))
}

fn explicit_keyboard_live_target_limit() -> usize {
    keyboard_targets()
        .iter()
        .filter(|target| {
            !target
                .name
                .chars()
                .all(|character| character.is_ascii_digit())
        })
        .count()
}

fn validate_keyboard_live_target_names(targets: &[String]) -> Result<(), String> {
    if targets.len() == 1 && targets[0].eq_ignore_ascii_case("all") {
        return Ok(());
    }
    let explicit_limit = explicit_keyboard_live_target_limit();
    if targets.len() > explicit_limit {
        return Err(format!(
            "keyboard live apply accepts at most {explicit_limit} explicit nonnumeric key names"
        ));
    }
    if targets
        .iter()
        .any(|target| target.eq_ignore_ascii_case("all"))
    {
        return Err("keyboard live target 'all' must be the sole token and value".into());
    }
    if targets
        .iter()
        .any(|target| target.chars().all(|character| character.is_ascii_digit()))
    {
        return Err("keyboard live apply requires named non-numeric keys".into());
    }
    let resolved =
        resolve_targets(RequestedDevice::Keyboard, targets).map_err(|error| error.to_string())?;
    if resolved.is_empty() || resolved.len() > explicit_limit {
        return Err(format!(
            "keyboard live apply requires 1..={explicit_limit} unique explicit nonnumeric keys"
        ));
    }
    Ok(())
}

fn resolve_targets(
    device: RequestedDevice,
    names: &[String],
) -> Result<Vec<&'static TargetDefinition>, Box<dyn Error>> {
    if names.iter().any(|name| name.eq_ignore_ascii_case("all")) && names.len() != 1 {
        return Err(cli_error(
            "target 'all' cannot be combined with explicit or repeated targets",
        ));
    }
    let names = names.iter().map(String::as_str).collect::<Vec<_>>();
    let result = match device {
        RequestedDevice::Keyboard => expand_keyboard_targets(&names),
        RequestedDevice::Chassis => expand_chassis_targets(&names),
    };
    match result {
        Ok(targets) => Ok(targets),
        Err(TargetError::UnknownTarget(name)) => {
            let other_device = match device {
                RequestedDevice::Keyboard if lookup_chassis_target(&name).is_some() => {
                    Some("chassis")
                }
                RequestedDevice::Chassis if lookup_keyboard_target(&name).is_some() => {
                    Some("keyboard")
                }
                _ => None,
            };
            if let Some(other_device) = other_device {
                Err(cli_error(format!(
                    "target '{name}' belongs to device '{other_device}', not '{}'",
                    device.as_str()
                )))
            } else {
                Err(cli_error(format!(
                    "unknown target '{name}' for device '{}'; run 'alienrgb zones --device {}'",
                    device.as_str(),
                    device.as_str()
                )))
            }
        }
        Err(error) => Err(Box::new(error)),
    }
}

fn validate_plan_gate(
    requested: RequestedDevice,
    dmi: &DmiIdentity,
    devices: &[DeviceSummary],
) -> Result<(), Box<dyn Error>> {
    if !dmi.supported {
        return Err(cli_error(
            "DMI gate failed: exact Alienware m16 R2 identity is required for a dry-run plan",
        ));
    }
    let (expected_vid, expected_pid) = match requested {
        RequestedDevice::Keyboard => ("0d62", "d2b1"),
        RequestedDevice::Chassis => ("187c", "0551"),
    };
    let device = devices
        .iter()
        .find(|device| {
            device.kind == requested.kind()
                && device.status == DeviceStatus::Found
                && device.vid.eq_ignore_ascii_case(expected_vid)
                && device.pid.eq_ignore_ascii_case(expected_pid)
        })
        .ok_or_else(|| {
            cli_error(format!(
                "USB gate failed: requested {expected_vid}:{expected_pid} {} controller is not present",
                requested.as_str()
            ))
        })?;
    if requested == RequestedDevice::Keyboard
        && !device
            .descriptor
            .as_ref()
            .is_some_and(|descriptor| descriptor.evidence == DescriptorEvidence::HashMatch)
    {
        return Err(cli_error(
            "keyboard gate failed: the confirmed descriptor hash match is required; a compatible signature is insufficient",
        ));
    }
    Ok(())
}

fn validate_power_profile_gate(
    dmi: &DmiIdentity,
    devices: &[DeviceSummary],
) -> Result<(), Box<dyn Error>> {
    validate_plan_gate(RequestedDevice::Chassis, dmi, devices)?;
    if dmi.bios_version.as_deref() != Some(CONFIRMED_BIOS_VERSION) {
        return Err(cli_error(
            "BIOS gate failed: power-profile requires exact BIOS 1.21.0",
        ));
    }
    Ok(())
}

fn zones_report(filter: Option<RequestedDevice>) -> ZonesReport {
    let mut zones = Vec::new();
    if filter.is_none() || filter == Some(RequestedDevice::Keyboard) {
        zones.extend(zone_entries(
            DeviceKind::Keyboard,
            "0d62:d2b1",
            keyboard_targets(),
        ));
    }
    if filter.is_none() || filter == Some(RequestedDevice::Chassis) {
        zones.extend(zone_entries(
            DeviceKind::Chassis,
            "187c:0551",
            chassis_targets(),
        ));
    }
    ZonesReport {
        schema_version: 1,
        command: CommandKind::Zones,
        usb_controller_count: 2,
        note: "Keyboard, touchpad, back/chassis, and power are logical RGB targets behind two USB controllers, not four USB devices.",
        zones,
    }
}

fn zone_entries(
    device: DeviceKind,
    usb_controller: &'static str,
    targets: &'static [TargetDefinition],
) -> Vec<ZoneSummary> {
    targets
        .iter()
        .map(|target| ZoneSummary {
            device,
            usb_controller,
            target: target.name,
            logical_id: target.logical_id,
            aliases: target.aliases,
            validation: target.validation,
            validation_detail: target.validation_detail,
            known_validation: target.known_validation,
        })
        .collect()
}

fn render<T: serde::Serialize>(
    json: bool,
    report: &T,
    human: impl FnOnce() -> String,
) -> Result<String, Box<dyn Error>> {
    Ok(if json {
        format!("{}\n", to_json(report)?)
    } else {
        human()
    })
}

fn with_missing(mut found: Vec<DeviceSummary>) -> Vec<DeviceSummary> {
    if !found
        .iter()
        .any(|device| device.kind == DeviceKind::Keyboard)
    {
        found.push(DeviceSummary::missing(DeviceKind::Keyboard));
    }
    if !found
        .iter()
        .any(|device| device.kind == DeviceKind::Chassis)
    {
        found.push(DeviceSummary::missing(DeviceKind::Chassis));
    }
    found.sort_by_key(|device| match device.kind {
        DeviceKind::Keyboard => 0,
        DeviceKind::Chassis => 1,
    });
    found
}

fn capabilities() -> Vec<CapabilityProfile> {
    vec![
        CapabilityProfile {
            kind: DeviceKind::Keyboard,
            name: "Per-key keyboard logical targets behind USB 0d62:d2b1",
            transport: "64-byte AlienFX API v5 hidapi feature operations through the exact interface-00 hidraw path; Linux 7.2.4 uses a 5-second USB control timeout per ioctl, without a formal cancellation deadline",
            logical_targets: keyboard_targets().iter().map(|target| target.name).collect(),
            write_support: "experimental_bounded_1_to_75_explicit_or_all_85_apply_effective_acl_access_available",
            validation_summary: "One-, two-, and six-frame static-red transport plus whole-keyboard coverage were physically validated on the exact Alienware m16 R2 / BIOS 1.21.0 / 0d62:d2b1 / pinned-descriptor profile; only Escape/F1/W/Space were individually discriminated. Other individual logical IDs, colors, effects, models, and BIOS versions remain unvalidated.",
        },
        CapabilityProfile {
            kind: DeviceKind::Chassis,
            name: "Touchpad, back/chassis, and power logical targets behind USB 187c:0551",
            transport: "33-byte direct-libusb AlienFX API v4 on wire; dangerous experimental single-zone live CLI routing requires three explicit flags; Windows HID uses a 34-byte caller buffer including report ID 0x00",
            logical_targets: chassis_targets().iter().map(|target| target.name).collect(),
            write_support: "experimental_apply_mixed_target_evidence",
            validation_summary: "Touchpad/haptic logical ID 0 and back/chassis logical ID 2 each have a separate exact static #ff0000 live validation on Alienware m16 R2 BIOS 1.21.0 with AW-ELC 187c:0551. The dedicated equal-color power-profile #ff0000 completed all 34 writes, with power red and no other zone change observed only during battery-on/discharging; the other five states remain visually unobserved. Ordinary power static/address semantics, other colors, effects, profiles, persistence, readback, and restore remain unvalidated.",
        },
    ]
}

fn doctor_report(dmi: &DmiIdentity, devices: &[DeviceSummary]) -> DoctorReport {
    let supported = dmi.supported;
    let confirmed_bios = dmi.bios_version.as_deref() == Some(CONFIRMED_BIOS_VERSION);
    let mut findings = vec![if supported {
        finding(
            "dmi_supported",
            FindingLevel::Ok,
            "DMI matches Alienware m16 R2.",
            None,
        )
    } else {
        finding(
            "dmi_unsupported",
            FindingLevel::Error,
            "DMI does not match the conservative Alienware m16 R2 profile.",
            Some("Run this tool only on the confirmed Alienware m16 R2 target."),
        )
    }];
    findings.push(if confirmed_bios {
        finding(
            "confirmed_bios_live_profile",
            FindingLevel::Ok,
            "BIOS 1.21.0 matches the confirmed live-write profile for both keyboard and AW-ELC.",
            None,
        )
    } else {
        finding(
            "confirmed_bios_live_profile",
            FindingLevel::Error,
            &format!(
                "BIOS {} does not match the confirmed live-write profile BIOS 1.21.0.",
                dmi.bios_version.as_deref().unwrap_or("unknown")
            ),
            Some("Do not perform live writes on an unconfirmed BIOS version."),
        )
    });

    for (kind, code) in [
        (DeviceKind::Keyboard, "keyboard_usb"),
        (DeviceKind::Chassis, "chassis_usb"),
    ] {
        let device = devices
            .iter()
            .find(|device| device.kind == kind)
            .expect("complete device list");
        findings.push(if device.status == DeviceStatus::Found {
            finding(
                code,
                FindingLevel::Ok,
                &format!("{:?} USB device found.", kind),
                None,
            )
        } else {
            finding(
                code,
                FindingLevel::Error,
                &format!("{:?} USB device is missing.", kind),
                Some("Check USB enumeration and confirm the device VID/PID with lsusb."),
            )
        });
    }

    let keyboard = devices
        .iter()
        .find(|device| device.kind == DeviceKind::Keyboard)
        .expect("complete device list");
    if keyboard.status == DeviceStatus::Found {
        let (level, message, action) =
            match keyboard.descriptor.as_ref().map(|status| status.evidence) {
                Some(DescriptorEvidence::HashMatch) => (
                    FindingLevel::Ok,
                    "Keyboard descriptor hash matches confirmed hardware.",
                    None,
                ),
                Some(DescriptorEvidence::CompatibleSignature) => (
                    FindingLevel::Warning,
                    "Keyboard descriptor has the compatible 0xFF89/0xCC 64-byte feature signature, but its hash differs.",
                    Some("Record and review the descriptor; live keyboard writes require the exact confirmed hash."),
                ),
                Some(DescriptorEvidence::Mismatch) => (
                    FindingLevel::Error,
                    "Keyboard descriptor does not have the expected vendor feature signature.",
                    Some("Do not perform live keyboard writes with this descriptor."),
                ),
                Some(DescriptorEvidence::Unreadable) => (
                    FindingLevel::Warning,
                    "Keyboard report descriptor exists but is not readable.",
                    Some("Check sysfs permissions; do not use sudo for alienrgb."),
                ),
                Some(DescriptorEvidence::NotFound) | None => (
                    FindingLevel::Warning,
                    "Keyboard report descriptor was not found in sysfs.",
                    Some("Confirm that the HID interface and hidraw driver are bound."),
                ),
            };
        findings.push(finding("keyboard_descriptor", level, message, action));
    }

    for device in devices.iter().filter(|device| {
        device.kind == DeviceKind::Keyboard && device.status == DeviceStatus::Found
    }) {
        let label = format!("{:?} hidraw", device.kind);
        if device.hidraw.is_empty() {
            findings.push(finding(
                "hidraw_missing",
                FindingLevel::Warning,
                &format!("{label} interface is not present."),
                Some("Confirm the hidraw kernel module and USB interface binding."),
            ));
        }
        for hidraw in &device.hidraw {
            let message = format!(
                "{}: node={}, mode_bits(read={}, write={}, basis={}); effective(read={}, write={}, basis={}).",
                hidraw.path,
                node_state(hidraw.node_state),
                access(hidraw.read_access),
                access(hidraw.write_access),
                hidraw.access_basis,
                effective_access(hidraw.effective_read_access),
                effective_access(hidraw.effective_write_access),
                hidraw.effective_access_basis
            );
            let ready = hidraw.node_state == HidrawNodeState::Present
                && hidraw.effective_read_access == EffectiveAccess::Allowed
                && hidraw.effective_write_access == EffectiveAccess::Allowed;
            let action = if hidraw.node_state == HidrawNodeState::Missing
                || matches!(hidraw.effective_read_access, EffectiveAccess::Missing)
                || matches!(hidraw.effective_write_access, EffectiveAccess::Missing)
            {
                Some("Confirm that udev created the hidraw device node; do not run alienrgb with sudo.")
            } else if matches!(
                (hidraw.effective_read_access, hidraw.effective_write_access),
                (EffectiveAccess::Unknown, _) | (_, EffectiveAccess::Unknown)
            ) {
                Some("Effective access could not be determined; inspect the active udev/ACL state without opening the node.")
            } else if !ready {
                Some("The active effective credentials cannot read and write the keyboard hidraw node; verify the installed narrow udev rule and session ACL without using sudo for alienrgb.")
            } else {
                None
            };
            findings.push(finding(
                "hidraw_permissions",
                if ready {
                    FindingLevel::Ok
                } else {
                    FindingLevel::Warning
                },
                &message,
                action,
            ));
        }
    }

    let ready_for_diagnostics = supported
        && devices
            .iter()
            .all(|device| device.status == DeviceStatus::Found)
        && keyboard.descriptor.as_ref().is_some_and(|status| {
            matches!(
                status.evidence,
                DescriptorEvidence::HashMatch | DescriptorEvidence::CompatibleSignature
            )
        });
    let keyboard_ready_for_live_write = supported
        && confirmed_bios
        && keyboard.status == DeviceStatus::Found
        && keyboard
            .descriptor
            .as_ref()
            .is_some_and(|status| status.evidence == DescriptorEvidence::HashMatch)
        && keyboard.hidraw.iter().any(|hidraw| {
            hidraw.interface_number.as_deref() == Some("00")
                && hidraw.node_state == HidrawNodeState::Present
                && hidraw.effective_read_access == EffectiveAccess::Allowed
                && hidraw.effective_write_access == EffectiveAccess::Allowed
        });
    let chassis_matches = devices
        .iter()
        .filter(|device| {
            device.kind == DeviceKind::Chassis
                && device.status == DeviceStatus::Found
                && device.vid.eq_ignore_ascii_case("187c")
                && device.pid.eq_ignore_ascii_case("0551")
        })
        .collect::<Vec<_>>();
    let chassis_ready_for_live_write = supported
        && confirmed_bios
        && chassis_matches.len() == 1
        && chassis_matches[0].bus_number.is_some()
        && chassis_matches[0]
            .port_path
            .as_ref()
            .is_some_and(|path| !path.is_empty())
        && chassis_matches[0]
            .interface_number
            .as_deref()
            .is_none_or(|interface| interface == "00");
    findings.push(if chassis_ready_for_live_write {
        finding(
            "chassis_live_profile",
            FindingLevel::Ok,
            "AW-ELC pre-open profile is technically ready: confirmed DMI/BIOS, one 187c:0551 controller, USB bus, physical port path, and no conflicting interface identity. Endpoint, driver, opened identity, and access checks are revalidated only during acquisition; this does not validate a physical target.",
            None,
        )
    } else {
        finding(
            "chassis_live_profile",
            FindingLevel::Error,
            "AW-ELC pre-open profile is not technically ready; it requires confirmed DMI/BIOS, exactly one 187c:0551 controller, a USB bus number, a nonempty physical port path, and interface 00 when interface identity is available.",
            Some("Resolve the missing or conflicting trusted-discovery evidence before any live AW-ELC execution."),
        )
    });
    DoctorReport {
        schema_version: 1,
        command: CommandKind::Doctor,
        ready_for_diagnostics,
        ready_for_writes: keyboard_ready_for_live_write && chassis_ready_for_live_write,
        keyboard_ready_for_live_write,
        chassis_ready_for_live_write,
        findings,
    }
}

fn finding(
    code: &'static str,
    level: FindingLevel,
    message: &str,
    action: Option<&str>,
) -> Finding {
    Finding {
        code,
        level,
        message: message.into(),
        action: action.map(str::to_string),
    }
}

fn access(value: PermissionEstimate) -> &'static str {
    match value {
        PermissionEstimate::AllowedByModeBits => "allowed_by_mode_bits",
        PermissionEstimate::DeniedByModeBits => "denied_by_mode_bits",
        PermissionEstimate::Unknown => "unknown",
    }
}

fn effective_access(value: EffectiveAccess) -> &'static str {
    match value {
        EffectiveAccess::Allowed => "allowed",
        EffectiveAccess::Denied => "denied",
        EffectiveAccess::Unknown => "unknown",
        EffectiveAccess::Missing => "missing",
    }
}

fn node_state(value: HidrawNodeState) -> &'static str {
    match value {
        HidrawNodeState::Present => "present",
        HidrawNodeState::Missing => "missing",
        HidrawNodeState::MetadataUnavailable => "metadata_unavailable",
    }
}

fn empty_dmi() -> DmiIdentity {
    DmiIdentity {
        vendor: None,
        product: None,
        bios_version: None,
        supported: false,
    }
}

fn cli_error(message: impl Into<String>) -> Box<dyn Error> {
    Box::new(io::Error::new(io::ErrorKind::InvalidInput, message.into()))
}
