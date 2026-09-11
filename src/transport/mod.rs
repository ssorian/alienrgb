mod aw_elc;
mod keyboard;

pub(crate) use aw_elc::RusbAwElcFactory;
pub(crate) use keyboard::HidApiKeyboardFactory;

#[cfg(test)]
mod tests;

use crate::model::{
    DeviceKind, DeviceStatus, DeviceSummary, DmiIdentity, PacketStep, ProtocolFamily, ProtocolPlan,
    TransferKind,
};
use crate::profile::{
    DescriptorEvidence, CONFIRMED_BIOS_VERSION, KEYBOARD_OBSERVED_STATUS_READY_SIGNATURE,
};
use crate::protocol::{api_v4, api_v5, power_v4, LogicalColor, Rgb};
use crate::targets::{keyboard_target_by_id, keyboard_targets};
use serde::Serialize;
use std::error::Error;
use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

const KEYBOARD_LENGTH: usize = 64;
const AW_ELC_LENGTH: usize = 33;
static KEYBOARD_EXECUTION_ACTIVE: AtomicBool = AtomicBool::new(false);
static AW_ELC_EXECUTION_ACTIVE: AtomicBool = AtomicBool::new(false);

pub(crate) struct KeyboardExecutionGuard;

impl KeyboardExecutionGuard {
    pub(crate) fn try_acquire() -> Result<Self, ExecutionError> {
        KEYBOARD_EXECUTION_ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| Self)
            .map_err(|_| {
                execution_error(
                    ExecutionErrorCode::Busy,
                    None,
                    "another keyboard execution is already active in this process",
                )
            })
    }
}

impl Drop for KeyboardExecutionGuard {
    fn drop(&mut self) {
        KEYBOARD_EXECUTION_ACTIVE.store(false, Ordering::Release);
    }
}

pub(crate) struct AwElcExecutionGuard;

impl AwElcExecutionGuard {
    pub(crate) fn try_acquire() -> Result<Self, ExecutionError> {
        AW_ELC_EXECUTION_ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| Self)
            .map_err(|_| {
                execution_error(
                    ExecutionErrorCode::Busy,
                    None,
                    "another AW-ELC execution is already active in this process",
                )
            })
    }
}

impl Drop for AwElcExecutionGuard {
    fn drop(&mut self) {
        AW_ELC_EXECUTION_ACTIVE.store(false, Ordering::Release);
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BackendError {
    code: &'static str,
}

impl BackendError {
    pub(crate) const fn new(code: &'static str) -> Self {
        Self { code }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExecutionErrorCode {
    InvalidFamily,
    InvalidTransfer,
    InvalidLength,
    InvalidStep,
    InvalidPayload,
    CanonicalMismatch,
    DiscoveryFailed,
    InvalidTarget,
    AmbiguousTarget,
    AcquisitionFailed,
    BackendUnavailable,
    Busy,
    BackendFailed,
    ShortTransfer,
    WaitUpdate,
    UnknownStatus,
    MalformedStatus,
    IdentityDrift,
    InterfaceDrift,
    EndpointDrift,
    DriverActive,
    DriverStateUnknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct ExecutionError {
    code: ExecutionErrorCode,
    step: Option<&'static str>,
    message: String,
    completed_steps: usize,
    transport_attempted: bool,
}

impl ExecutionError {
    pub(crate) fn code_text(&self) -> String {
        serde_json::to_value(self.code)
            .ok()
            .and_then(|value| value.as_str().map(str::to_string))
            .unwrap_or_else(|| "unknown_execution_error".to_string())
    }
    pub(crate) fn step(&self) -> Option<&'static str> {
        self.step
    }
    pub(crate) fn completed_steps(&self) -> usize {
        self.completed_steps
    }
    pub(crate) fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for ExecutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for ExecutionError {}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct ExecutedStep {
    name: &'static str,
    transfer: TransferKind,
    expected_length: usize,
    actual_length: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct ExecutionResult {
    family: ProtocolFamily,
    completed: bool,
    status_byte: Option<u8>,
    steps: Vec<ExecutedStep>,
}

impl ExecutionResult {
    pub(crate) fn completed(&self) -> bool {
        self.completed
    }

    pub(crate) fn steps(&self) -> &[ExecutedStep] {
        &self.steps
    }
}

impl ExecutedStep {
    pub(crate) fn name(&self) -> &'static str {
        self.name
    }

    pub(crate) fn actual_length(&self) -> usize {
        self.actual_length
    }
}

pub(crate) trait FreshDiscovery {
    fn discover(&mut self) -> Result<(DmiIdentity, Vec<DeviceSummary>), BackendError>;
}

pub(crate) struct SystemDiscovery;

impl FreshDiscovery for SystemDiscovery {
    fn discover(&mut self) -> Result<(DmiIdentity, Vec<DeviceSummary>), BackendError> {
        crate::sysfs::discover_for_transport()
            .map_err(|_| BackendError::new("trusted_discovery_failed"))
    }
}

pub(crate) trait KeyboardBackend {
    fn send_feature_report(&mut self, data: &[u8]) -> Result<usize, BackendError>;
    fn get_feature_report(&mut self, data: &mut [u8]) -> Result<usize, BackendError>;
}

pub(crate) trait KeyboardBackendFactory {
    fn requires_process_guard(&self) -> bool {
        false
    }

    fn acquire(
        &mut self,
        selection: &KeyboardSelection,
    ) -> Result<Box<dyn KeyboardBackend>, BackendError>;
}

pub(crate) trait AwElcBackend {
    fn interrupt_write(&mut self, data: &[u8]) -> Result<usize, BackendError>;
}

pub(crate) trait AwElcBackendFactory {
    fn requires_process_guard(&self) -> bool {
        false
    }

    fn acquire(
        &mut self,
        selection: &AwElcSelection,
    ) -> Result<Box<dyn AwElcBackend>, BackendError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct KeyboardStatusCapture {
    pub(crate) query_write_length: usize,
    pub(crate) response: Vec<u8>,
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedKeyboardExecution {
    intent: Vec<LogicalColor>,
    canonical: ProtocolPlan,
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedAwElcExecution {
    intent: Vec<LogicalColor>,
    canonical: ProtocolPlan,
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedPowerProfileExecution {
    color: Rgb,
    canonical: ProtocolPlan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SetAllLiveIntent {
    pub(crate) color: Rgb,
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedSetAllExecution {
    intent: SetAllLiveIntent,
    keyboard: PreparedKeyboardExecution,
    aw_static: PreparedAwElcExecution,
    power_profile: PreparedPowerProfileExecution,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CompoundExecutionStage {
    pub(crate) name: &'static str,
    pub(crate) expected_steps: usize,
    pub(crate) completed_steps: usize,
    pub(crate) transport_attempted: bool,
    pub(crate) failure: Option<ExecutionError>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SetAllExecutionOutcome {
    pub(crate) stages: Vec<CompoundExecutionStage>,
}

impl SetAllExecutionOutcome {
    #[cfg(test)]
    pub(crate) fn successful() -> Self {
        Self {
            stages: vec![
                compound_completed("keyboard", 11, 11),
                compound_completed("aw_static_touchpad_back", 4, 4),
                compound_completed("power_profile", 34, 34),
            ],
        }
    }

    #[cfg(test)]
    pub(crate) fn failed_for_test(stage_index: usize, completed_steps: usize) -> Self {
        let specs = [
            ("keyboard", 11),
            ("aw_static_touchpad_back", 4),
            ("power_profile", 34),
        ];
        let mut stages = Vec::new();
        for (index, (name, expected)) in specs.into_iter().enumerate() {
            if index < stage_index {
                stages.push(compound_completed(name, expected, expected));
            } else if index == stage_index {
                let mut error = execution_error(
                    ExecutionErrorCode::BackendFailed,
                    Some("injected_step"),
                    "injected failure",
                );
                error.completed_steps = completed_steps;
                error.transport_attempted = true;
                stages.push(CompoundExecutionStage {
                    name,
                    expected_steps: expected,
                    completed_steps,
                    transport_attempted: true,
                    failure: Some(error),
                });
            } else {
                stages.push(CompoundExecutionStage {
                    name,
                    expected_steps: expected,
                    completed_steps: 0,
                    transport_attempted: false,
                    failure: None,
                });
            }
        }
        Self { stages }
    }

    pub(crate) fn completed(&self) -> bool {
        self.stages
            .iter()
            .all(|stage| stage.failure.is_none() && stage.completed_steps == stage.expected_steps)
    }

    pub(crate) fn transport_attempted(&self) -> bool {
        self.stages.iter().any(|stage| stage.transport_attempted)
    }

    pub(crate) fn transport_performed(&self) -> bool {
        self.transport_attempted() || self.stages.iter().any(|stage| stage.completed_steps > 0)
    }
}

pub(crate) fn prepare_keyboard(
    intent: Vec<LogicalColor>,
) -> Result<PreparedKeyboardExecution, ExecutionError> {
    if intent.is_empty() || intent.len() > keyboard_targets().len() {
        return Err(invalid_target(format!(
            "keyboard live intent requires 1..={} assignments",
            keyboard_targets().len()
        )));
    }
    if intent
        .iter()
        .any(|assignment| keyboard_target_by_id(assignment.logical_id).is_none())
    {
        return Err(invalid_target(
            "keyboard live intent contains an unknown logical ID",
        ));
    }
    if intent
        .windows(2)
        .any(|pair| pair[0].logical_id >= pair[1].logical_id)
    {
        return Err(invalid_target(
            "keyboard live intent must contain unique logical IDs in canonical order",
        ));
    }
    let common_color = intent[0].color;
    if intent
        .iter()
        .any(|assignment| assignment.color != common_color)
    {
        return Err(invalid_target(
            "keyboard live intent requires exactly one common color",
        ));
    }
    let canonical = api_v5::encode_static(&intent).map_err(|_| {
        execution_error(
            ExecutionErrorCode::InvalidTarget,
            None,
            "keyboard intent contains an invalid known-target assignment",
        )
    })?;
    let expected_color_frames = intent.len().div_ceil(15);
    if canonical
        .steps()
        .iter()
        .filter(|step| step.name == "set_color")
        .count()
        != expected_color_frames
    {
        return Err(execution_error(
            ExecutionErrorCode::InvalidStep,
            Some("set_color"),
            format!(
                "keyboard live intent must encode exactly {expected_color_frames} color frames"
            ),
        ));
    }
    Ok(PreparedKeyboardExecution { intent, canonical })
}

pub(crate) fn prepare_aw_elc(
    intent: Vec<LogicalColor>,
) -> Result<PreparedAwElcExecution, ExecutionError> {
    let canonical = api_v4::encode_static(&intent).map_err(|_| {
        execution_error(
            ExecutionErrorCode::InvalidTarget,
            None,
            "AW-ELC intent contains an invalid known-target assignment",
        )
    })?;
    Ok(PreparedAwElcExecution { intent, canonical })
}

pub(crate) fn prepare_power_profile(color: Rgb) -> PreparedPowerProfileExecution {
    PreparedPowerProfileExecution {
        color,
        canonical: power_v4::encode_equal_color(color),
    }
}

pub(crate) fn prepare_set_all(
    intent: SetAllLiveIntent,
) -> Result<PreparedSetAllExecution, ExecutionError> {
    let keyboard = prepare_keyboard(
        keyboard_targets()
            .iter()
            .map(|target| LogicalColor::new(target.logical_id, intent.color))
            .collect(),
    )?;
    let aw_static = prepare_aw_elc(vec![
        LogicalColor::new(0, intent.color),
        LogicalColor::new(2, intent.color),
    ])?;
    let power_profile = prepare_power_profile(intent.color);
    Ok(PreparedSetAllExecution {
        intent,
        keyboard,
        aw_static,
        power_profile,
    })
}

pub(crate) fn execute_set_all(
    prepared: PreparedSetAllExecution,
    discovery: &mut impl FreshDiscovery,
    keyboard_factory: &mut impl KeyboardBackendFactory,
    aw_factory: &mut impl AwElcBackendFactory,
) -> Result<SetAllExecutionOutcome, ExecutionError> {
    if prepared.intent.color != prepared.keyboard.intent[0].color
        || prepared.intent.color != prepared.power_profile.color
        || prepared
            .aw_static
            .intent
            .iter()
            .any(|assignment| assignment.color != prepared.intent.color)
    {
        return Err(execution_error(
            ExecutionErrorCode::CanonicalMismatch,
            None,
            "set-all prepared intents do not share the requested color",
        ));
    }
    let keyboard_plan = regenerate_keyboard(&prepared.keyboard)?;
    let keyboard_frames = validate_keyboard_plan(&keyboard_plan)?;
    let aw_plan = regenerate_aw_elc(&prepared.aw_static)?;
    let aw_frames = validate_aw_elc_plan(&aw_plan)?;
    let power_plan = regenerate_power_profile(&prepared.power_profile)?;
    let power_frames = validate_power_profile_plan(&power_plan, prepared.intent.color)?;

    let _keyboard_guard = if keyboard_factory.requires_process_guard() {
        Some(KeyboardExecutionGuard::try_acquire()?)
    } else {
        None
    };
    let _aw_guard = if aw_factory.requires_process_guard() {
        Some(AwElcExecutionGuard::try_acquire()?)
    } else {
        None
    };
    let mut stages = Vec::with_capacity(3);

    match execute_keyboard_lock_held(&keyboard_plan, keyboard_frames, discovery, keyboard_factory) {
        Ok(result) => stages.push(compound_completed("keyboard", 11, result.steps.len())),
        Err(error) => {
            return Ok(compound_failed(
                stages,
                "keyboard",
                11,
                error,
                &[("aw_static_touchpad_back", 4), ("power_profile", 34)],
            ))
        }
    }
    match execute_aw_elc_lock_held(&aw_plan, aw_frames, discovery, aw_factory) {
        Ok(result) => stages.push(compound_completed(
            "aw_static_touchpad_back",
            4,
            result.steps.len(),
        )),
        Err(error) => {
            return Ok(compound_failed(
                stages,
                "aw_static_touchpad_back",
                4,
                error,
                &[("power_profile", 34)],
            ))
        }
    }
    match execute_aw_elc_lock_held(&power_plan, power_frames, discovery, aw_factory) {
        Ok(result) => stages.push(compound_completed("power_profile", 34, result.steps.len())),
        Err(error) => return Ok(compound_failed(stages, "power_profile", 34, error, &[])),
    }
    Ok(SetAllExecutionOutcome { stages })
}

fn compound_completed(
    name: &'static str,
    expected_steps: usize,
    completed_steps: usize,
) -> CompoundExecutionStage {
    CompoundExecutionStage {
        name,
        expected_steps,
        completed_steps,
        transport_attempted: completed_steps > 0,
        failure: None,
    }
}

fn compound_failed(
    mut stages: Vec<CompoundExecutionStage>,
    name: &'static str,
    expected_steps: usize,
    error: ExecutionError,
    later: &[(&'static str, usize)],
) -> SetAllExecutionOutcome {
    stages.push(CompoundExecutionStage {
        name,
        expected_steps,
        completed_steps: error.completed_steps,
        transport_attempted: error.transport_attempted,
        failure: Some(error),
    });
    stages.extend(
        later
            .iter()
            .map(|(name, expected_steps)| CompoundExecutionStage {
                name,
                expected_steps: *expected_steps,
                completed_steps: 0,
                transport_attempted: false,
                failure: None,
            }),
    );
    SetAllExecutionOutcome { stages }
}

pub(crate) fn execute_keyboard(
    prepared: PreparedKeyboardExecution,
    discovery: &mut impl FreshDiscovery,
    factory: &mut impl KeyboardBackendFactory,
) -> Result<ExecutionResult, ExecutionError> {
    let canonical = regenerate_keyboard(&prepared)?;
    let frames = validate_keyboard_plan(&canonical)?;
    let _execution_guard = if factory.requires_process_guard() {
        Some(KeyboardExecutionGuard::try_acquire()?)
    } else {
        None
    };
    execute_keyboard_lock_held(&canonical, frames, discovery, factory)
}

fn execute_keyboard_lock_held(
    canonical: &ProtocolPlan,
    frames: Vec<ValidatedFrame<'_>>,
    discovery: &mut impl FreshDiscovery,
    factory: &mut impl KeyboardBackendFactory,
) -> Result<ExecutionResult, ExecutionError> {
    let (dmi, devices) = fresh_discovery(discovery)?;
    let selection = select_keyboard(&dmi, &devices)?;
    validate_keyboard_live_profile(&dmi)?;
    let mut backend = factory.acquire(&selection).map_err(map_acquisition_error)?;
    dispatch_keyboard(canonical, frames, &mut *backend).map_err(mark_transport_attempted)
}

pub(crate) fn execute_keyboard_status(
    discovery: &mut impl FreshDiscovery,
    factory: &mut impl KeyboardBackendFactory,
) -> Result<KeyboardStatusCapture, ExecutionError> {
    let _execution_guard = if factory.requires_process_guard() {
        Some(KeyboardExecutionGuard::try_acquire()?)
    } else {
        None
    };
    let (dmi, devices) = fresh_discovery(discovery)?;
    let selection = select_keyboard(&dmi, &devices)?;
    let mut backend = factory.acquire(&selection).map_err(map_acquisition_error)?;
    let (query, mut response) = api_v5::status_query_buffers();
    let written = backend
        .send_feature_report(&query)
        .map_err(|error| backend_error("query_status", error))?;
    if written != KEYBOARD_LENGTH {
        return Err(short_transfer("query_status", KEYBOARD_LENGTH, written));
    }
    let actual = backend
        .get_feature_report(&mut response)
        .map_err(|error| backend_error("read_status", error))?;
    if actual == 0 || actual > KEYBOARD_LENGTH {
        return Err(execution_error(
            ExecutionErrorCode::ShortTransfer,
            Some("read_status"),
            format!("keyboard status capture returned invalid length {actual}"),
        ));
    }
    if response[0] != 0xcc {
        return Err(execution_error(
            ExecutionErrorCode::MalformedStatus,
            Some("read_status"),
            "keyboard status response has an invalid report ID",
        ));
    }
    Ok(KeyboardStatusCapture {
        query_write_length: written,
        response: response[..actual].to_vec(),
    })
}

pub(crate) fn execute_power_profile(
    prepared: PreparedPowerProfileExecution,
    discovery: &mut impl FreshDiscovery,
    factory: &mut impl AwElcBackendFactory,
) -> Result<ExecutionResult, ExecutionError> {
    let canonical = regenerate_power_profile(&prepared)?;
    let frames = validate_power_profile_plan(&canonical, prepared.color)?;
    let _execution_guard = if factory.requires_process_guard() {
        Some(AwElcExecutionGuard::try_acquire()?)
    } else {
        None
    };
    execute_aw_elc_lock_held(&canonical, frames, discovery, factory)
}

pub(crate) fn execute_aw_elc(
    prepared: PreparedAwElcExecution,
    discovery: &mut impl FreshDiscovery,
    factory: &mut impl AwElcBackendFactory,
) -> Result<ExecutionResult, ExecutionError> {
    let canonical = regenerate_aw_elc(&prepared)?;
    let frames = validate_aw_elc_plan(&canonical)?;
    let _execution_guard = if factory.requires_process_guard() {
        Some(AwElcExecutionGuard::try_acquire()?)
    } else {
        None
    };
    execute_aw_elc_lock_held(&canonical, frames, discovery, factory)
}

fn execute_aw_elc_lock_held(
    canonical: &ProtocolPlan,
    frames: Vec<ValidatedFrame<'_>>,
    discovery: &mut impl FreshDiscovery,
    factory: &mut impl AwElcBackendFactory,
) -> Result<ExecutionResult, ExecutionError> {
    let (dmi, devices) = fresh_discovery(discovery)?;
    let selection = select_aw_elc(&dmi, &devices)?;
    validate_aw_elc_live_profile(&dmi)?;
    let mut backend = factory.acquire(&selection).map_err(map_acquisition_error)?;
    dispatch_aw_elc(canonical, frames, &mut *backend).map_err(mark_transport_attempted)
}

fn regenerate_keyboard(
    prepared: &PreparedKeyboardExecution,
) -> Result<ProtocolPlan, ExecutionError> {
    let canonical = api_v5::encode_static(&prepared.intent).map_err(|_| {
        execution_error(
            ExecutionErrorCode::InvalidTarget,
            None,
            "keyboard intent failed canonical regeneration",
        )
    })?;
    if canonical != prepared.canonical {
        return Err(execution_error(
            ExecutionErrorCode::CanonicalMismatch,
            None,
            "prepared keyboard plan differs from canonical encoder output",
        ));
    }
    Ok(canonical)
}

fn regenerate_aw_elc(prepared: &PreparedAwElcExecution) -> Result<ProtocolPlan, ExecutionError> {
    let canonical = api_v4::encode_static(&prepared.intent).map_err(|_| {
        execution_error(
            ExecutionErrorCode::InvalidTarget,
            None,
            "AW-ELC intent failed canonical regeneration",
        )
    })?;
    if canonical != prepared.canonical {
        return Err(execution_error(
            ExecutionErrorCode::CanonicalMismatch,
            None,
            "prepared AW-ELC plan differs from canonical encoder output",
        ));
    }
    Ok(canonical)
}

fn regenerate_power_profile(
    prepared: &PreparedPowerProfileExecution,
) -> Result<ProtocolPlan, ExecutionError> {
    let canonical = power_v4::encode_equal_color(prepared.color);
    if canonical != prepared.canonical {
        return Err(execution_error(
            ExecutionErrorCode::CanonicalMismatch,
            None,
            "prepared power-profile plan differs from canonical encoder output",
        ));
    }
    Ok(canonical)
}

fn validate_keyboard_live_profile(dmi: &DmiIdentity) -> Result<(), ExecutionError> {
    if dmi.bios_version.as_deref() != Some(CONFIRMED_BIOS_VERSION) {
        return Err(invalid_target(
            "keyboard live status readiness is pinned to Alienware m16 R2 BIOS 1.21.0",
        ));
    }
    Ok(())
}

fn validate_aw_elc_live_profile(dmi: &DmiIdentity) -> Result<(), ExecutionError> {
    if dmi.bios_version.as_deref() != Some(CONFIRMED_BIOS_VERSION) {
        return Err(invalid_target(
            "AW-ELC live execution is pinned to Alienware m16 R2 BIOS 1.21.0",
        ));
    }
    Ok(())
}

fn fresh_discovery(
    discovery: &mut impl FreshDiscovery,
) -> Result<(DmiIdentity, Vec<DeviceSummary>), ExecutionError> {
    discovery.discover().map_err(|_| {
        execution_error(
            ExecutionErrorCode::DiscoveryFailed,
            None,
            "fresh trusted DMI/USB discovery failed",
        )
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct KeyboardSelection {
    path: PathBuf,
}

fn select_keyboard(
    dmi: &DmiIdentity,
    devices: &[DeviceSummary],
) -> Result<KeyboardSelection, ExecutionError> {
    if !dmi.supported {
        return Err(invalid_target("fresh DMI does not match Alienware m16 R2"));
    }
    let matching = devices
        .iter()
        .filter(|device| {
            device.kind == DeviceKind::Keyboard
                && device.status == DeviceStatus::Found
                && device.vid.eq_ignore_ascii_case("0d62")
                && device.pid.eq_ignore_ascii_case("d2b1")
        })
        .collect::<Vec<_>>();
    if matching.len() != 1 {
        return Err(execution_error(
            ExecutionErrorCode::AmbiguousTarget,
            None,
            "fresh discovery did not find exactly one keyboard controller",
        ));
    }
    let device = matching[0];
    if !device.descriptor.as_ref().is_some_and(|descriptor| {
        descriptor.evidence == DescriptorEvidence::HashMatch
            && descriptor.interface_number.as_deref() == Some("00")
    }) {
        return Err(invalid_target(
            "fresh keyboard descriptor is not the confirmed interface-00 hash",
        ));
    }
    let paths = device
        .hidraw
        .iter()
        .filter(|hidraw| hidraw.interface_number.as_deref() == Some("00"))
        .collect::<Vec<_>>();
    if paths.len() != 1 {
        return Err(execution_error(
            ExecutionErrorCode::AmbiguousTarget,
            None,
            "fresh discovery did not find exactly one interface-00 keyboard hidraw path",
        ));
    }
    Ok(KeyboardSelection {
        path: PathBuf::from(&paths[0].path),
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AwElcSelection {
    bus_number: u8,
    port_path: Vec<u8>,
    serial: Option<String>,
}

fn select_aw_elc(
    dmi: &DmiIdentity,
    devices: &[DeviceSummary],
) -> Result<AwElcSelection, ExecutionError> {
    if !dmi.supported {
        return Err(invalid_target("fresh DMI does not match Alienware m16 R2"));
    }
    let matching = devices
        .iter()
        .filter(|device| {
            device.kind == DeviceKind::Chassis
                && device.status == DeviceStatus::Found
                && device.vid.eq_ignore_ascii_case("187c")
                && device.pid.eq_ignore_ascii_case("0551")
        })
        .collect::<Vec<_>>();
    if matching.len() != 1 {
        return Err(execution_error(
            ExecutionErrorCode::AmbiguousTarget,
            None,
            "fresh discovery did not find exactly one AW-ELC controller",
        ));
    }
    let device = matching[0];
    let bus_number = device
        .bus_number
        .ok_or_else(|| invalid_target("fresh AW-ELC discovery lacks a USB bus number"))?;
    let port_path = device
        .port_path
        .clone()
        .filter(|path| !path.is_empty())
        .ok_or_else(|| invalid_target("fresh AW-ELC discovery lacks a physical USB port path"))?;
    Ok(AwElcSelection {
        bus_number,
        port_path,
        serial: device.serial.clone(),
    })
}

fn dispatch_keyboard(
    plan: &ProtocolPlan,
    frames: Vec<ValidatedFrame<'_>>,
    backend: &mut dyn KeyboardBackend,
) -> Result<ExecutionResult, ExecutionError> {
    let mut result = ExecutionResult {
        family: plan.family,
        completed: false,
        status_byte: None,
        steps: Vec::with_capacity(frames.len()),
    };
    for frame in frames {
        let completed = result.steps.len();
        let actual_length = match frame.step.transfer {
            TransferKind::HidFeatureWrite => {
                backend.send_feature_report(&frame.bytes).map_err(|error| {
                    with_completed_steps(backend_error(frame.step.name, error), completed)
                })?
            }
            TransferKind::HidFeatureReadIntent => {
                let mut response = frame.bytes;
                response[0] = 0xcc;
                let actual = backend.get_feature_report(&mut response).map_err(|error| {
                    with_completed_steps(backend_error(frame.step.name, error), completed)
                })?;
                if actual != KEYBOARD_OBSERVED_STATUS_READY_SIGNATURE.len() {
                    return Err(short_transfer(
                        frame.step.name,
                        KEYBOARD_OBSERVED_STATUS_READY_SIGNATURE.len(),
                        actual,
                    ));
                }
                let response = &response[..actual];
                if response[0] != 0xcc {
                    return Err(execution_error(
                        ExecutionErrorCode::MalformedStatus,
                        Some(frame.step.name),
                        "keyboard status response has an invalid report ID",
                    ));
                }
                if response[1] != 0x93 {
                    return Err(execution_error(
                        ExecutionErrorCode::MalformedStatus,
                        Some(frame.step.name),
                        "keyboard status response has an invalid query opcode",
                    ));
                }
                let status = response[2];
                result.status_byte = Some(status);
                if status == 0x80 {
                    return Err(execution_error(
                        ExecutionErrorCode::WaitUpdate,
                        Some(frame.step.name),
                        "keyboard reported WAITUPDATE; color frames were not sent",
                    ));
                }
                if response != KEYBOARD_OBSERVED_STATUS_READY_SIGNATURE {
                    let response_hex = response
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect::<String>();
                    return Err(execution_error(
                        ExecutionErrorCode::UnknownStatus,
                        Some(frame.step.name),
                        format!("keyboard returned unsupported status signature {response_hex}"),
                    ));
                }
                actual
            }
            TransferKind::UsbOutput => unreachable!("validated keyboard transfer"),
        };
        if frame.step.transfer == TransferKind::HidFeatureWrite && actual_length != KEYBOARD_LENGTH
        {
            return Err(short_transfer(
                frame.step.name,
                KEYBOARD_LENGTH,
                actual_length,
            ));
        }
        result.steps.push(executed_step(frame.step, actual_length));
    }
    result.completed = true;
    Ok(result)
}

fn dispatch_aw_elc(
    plan: &ProtocolPlan,
    frames: Vec<ValidatedFrame<'_>>,
    backend: &mut dyn AwElcBackend,
) -> Result<ExecutionResult, ExecutionError> {
    let mut result = ExecutionResult {
        family: plan.family,
        completed: false,
        status_byte: None,
        steps: Vec::with_capacity(frames.len()),
    };
    for frame in frames {
        let completed = result.steps.len();
        let actual_length = backend.interrupt_write(&frame.bytes).map_err(|error| {
            with_completed_steps(backend_error(frame.step.name, error), completed)
        })?;
        if actual_length != AW_ELC_LENGTH {
            return Err(with_completed_steps(
                short_transfer(frame.step.name, AW_ELC_LENGTH, actual_length),
                completed,
            ));
        }
        result.steps.push(executed_step(frame.step, actual_length));
    }
    result.completed = true;
    Ok(result)
}

fn executed_step(step: &PacketStep, actual_length: usize) -> ExecutedStep {
    ExecutedStep {
        name: step.name,
        transfer: step.transfer,
        expected_length: step.on_wire_length,
        actual_length,
    }
}

#[derive(Debug)]
struct ValidatedFrame<'a> {
    step: &'a PacketStep,
    bytes: Vec<u8>,
}

fn validate_keyboard_plan(plan: &ProtocolPlan) -> Result<Vec<ValidatedFrame<'_>>, ExecutionError> {
    validate_plan(
        plan,
        ProtocolFamily::AlienFxApiV5,
        KEYBOARD_LENGTH,
        &["reset", "query_status", "read_status"],
        &["loop", "update"],
        |step| {
            if step.name == "read_status" {
                TransferKind::HidFeatureReadIntent
            } else {
                TransferKind::HidFeatureWrite
            }
        },
        valid_keyboard_prefix,
    )
}

fn validate_power_profile_plan(
    plan: &ProtocolPlan,
    color: Rgb,
) -> Result<Vec<ValidatedFrame<'_>>, ExecutionError> {
    let expected = power_v4::encode_equal_color(color);
    if plan.family != ProtocolFamily::AlienFxApiV4 {
        return Err(execution_error(
            ExecutionErrorCode::InvalidFamily,
            None,
            "power-profile plan has the wrong protocol family",
        ));
    }
    if plan.steps.len() != 34 {
        return Err(execution_error(
            ExecutionErrorCode::InvalidStep,
            None,
            "power-profile plan must contain exactly 34 ordered steps",
        ));
    }
    plan.steps
        .iter()
        .zip(expected.steps.iter())
        .map(|(step, expected_step)| {
            if step.name != expected_step.name {
                return Err(step_error(
                    ExecutionErrorCode::InvalidStep,
                    step,
                    "power-profile step name or order is invalid",
                ));
            }
            if step.transfer != TransferKind::UsbOutput {
                return Err(step_error(
                    ExecutionErrorCode::InvalidTransfer,
                    step,
                    "power-profile steps must use USB output transfers",
                ));
            }
            if step.caller_buffer_length != AW_ELC_LENGTH || step.on_wire_length != AW_ELC_LENGTH {
                return Err(step_error(
                    ExecutionErrorCode::InvalidLength,
                    step,
                    "power-profile frames must be exactly 33 bytes",
                ));
            }
            let bytes = decode_payload(step, AW_ELC_LENGTH)?;
            let expected_bytes = decode_payload(expected_step, AW_ELC_LENGTH)?;
            if bytes != expected_bytes {
                return Err(step_error(
                    ExecutionErrorCode::InvalidPayload,
                    step,
                    "power-profile frame differs from the canonical payload",
                ));
            }
            Ok(ValidatedFrame { step, bytes })
        })
        .collect()
}

fn validate_aw_elc_plan(plan: &ProtocolPlan) -> Result<Vec<ValidatedFrame<'_>>, ExecutionError> {
    validate_plan(
        plan,
        ProtocolFamily::AlienFxApiV4,
        AW_ELC_LENGTH,
        &["remove", "start"],
        &["finish_play"],
        |_| TransferKind::UsbOutput,
        valid_aw_elc_prefix,
    )
}

fn validate_plan<'a>(
    plan: &'a ProtocolPlan,
    family: ProtocolFamily,
    length: usize,
    prefix: &[&str],
    suffix: &[&str],
    transfer_for: impl Fn(&PacketStep) -> TransferKind,
    prefix_valid: impl Fn(&str, &[u8]) -> bool,
) -> Result<Vec<ValidatedFrame<'a>>, ExecutionError> {
    if plan.family != family {
        return Err(execution_error(
            ExecutionErrorCode::InvalidFamily,
            None,
            "canonical plan has the wrong protocol family",
        ));
    }
    validate_sequence(&plan.steps, prefix, suffix)?;
    plan.steps
        .iter()
        .map(|step| {
            if step.transfer != transfer_for(step) {
                return Err(step_error(
                    ExecutionErrorCode::InvalidTransfer,
                    step,
                    "canonical plan contains an unsupported transfer kind",
                ));
            }
            if step.caller_buffer_length != length || step.on_wire_length != length {
                return Err(step_error(
                    ExecutionErrorCode::InvalidLength,
                    step,
                    "canonical frame length does not match the transport contract",
                ));
            }
            let bytes = decode_payload(step, length)?;
            if !prefix_valid(step.name, &bytes) {
                return Err(step_error(
                    ExecutionErrorCode::InvalidPayload,
                    step,
                    "canonical frame bytes do not match the step contract",
                ));
            }
            Ok(ValidatedFrame { step, bytes })
        })
        .collect()
}

fn validate_sequence(
    steps: &[PacketStep],
    prefix: &[&str],
    suffix: &[&str],
) -> Result<(), ExecutionError> {
    if steps.len() < prefix.len() + suffix.len() + 1 {
        return Err(execution_error(
            ExecutionErrorCode::InvalidStep,
            None,
            "canonical plan does not contain the required ordered steps",
        ));
    }
    let suffix_start = steps.len() - suffix.len();
    if steps
        .iter()
        .zip(prefix)
        .any(|(step, name)| step.name != *name)
        || steps[suffix_start..]
            .iter()
            .zip(suffix)
            .any(|(step, name)| step.name != *name)
        || steps[prefix.len()..suffix_start]
            .iter()
            .any(|step| step.name != "set_color")
    {
        return Err(execution_error(
            ExecutionErrorCode::InvalidStep,
            None,
            "canonical plan step count or order is invalid",
        ));
    }
    Ok(())
}

fn decode_payload(step: &PacketStep, expected: usize) -> Result<Vec<u8>, ExecutionError> {
    let payload = step.payload_hex.as_deref().ok_or_else(|| {
        step_error(
            ExecutionErrorCode::InvalidPayload,
            step,
            "canonical frame is missing hexadecimal payload data",
        )
    })?;
    if payload.len() != expected * 2 || !payload.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(step_error(
            ExecutionErrorCode::InvalidPayload,
            step,
            "canonical hexadecimal payload has an invalid length or character",
        ));
    }
    (0..payload.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&payload[index..index + 2], 16).map_err(|_| {
                step_error(
                    ExecutionErrorCode::InvalidPayload,
                    step,
                    "canonical hexadecimal payload is invalid",
                )
            })
        })
        .collect()
}

fn valid_keyboard_prefix(name: &str, bytes: &[u8]) -> bool {
    match name {
        "reset" => bytes.starts_with(&[0xcc, 0x94]),
        "query_status" => bytes.starts_with(&[0xcc, 0x93]),
        "read_status" => bytes.first() == Some(&0xcc),
        "set_color" => bytes.starts_with(&[0xcc, 0x8c, 0x02, 0x00]),
        "loop" => bytes.starts_with(&[0xcc, 0x8c, 0x13]),
        "update" => bytes.starts_with(&[0xcc, 0x8b, 0x01, 0xff]),
        _ => false,
    }
}

fn valid_aw_elc_prefix(name: &str, bytes: &[u8]) -> bool {
    match name {
        "remove" => bytes.starts_with(&[0x03, 0x21, 0x00, 0x04, 0xff, 0xff]),
        "start" => bytes.starts_with(&[0x03, 0x21, 0x00, 0x01, 0xff, 0xff]),
        "set_color" => bytes.starts_with(&[0x03, 0x27]),
        "finish_play" => bytes.starts_with(&[0x03, 0x21, 0x00, 0x03, 0xff, 0xff]),
        _ => false,
    }
}

fn map_acquisition_error(error: BackendError) -> ExecutionError {
    let code = if error.code == "keyboard_timeout_strategy_unavailable" {
        ExecutionErrorCode::BackendUnavailable
    } else {
        ExecutionErrorCode::AcquisitionFailed
    };
    execution_error(
        code,
        None,
        format!("transport acquisition failed ({})", error.code),
    )
}

fn backend_error(step: &'static str, error: BackendError) -> ExecutionError {
    execution_error(
        ExecutionErrorCode::BackendFailed,
        Some(step),
        format!("transport backend failed at step '{step}' ({})", error.code),
    )
}

fn short_transfer(step: &'static str, expected: usize, actual: usize) -> ExecutionError {
    execution_error(
        ExecutionErrorCode::ShortTransfer,
        Some(step),
        format!("step '{step}' transferred {actual} bytes; expected exactly {expected}"),
    )
}

fn mark_transport_attempted(mut error: ExecutionError) -> ExecutionError {
    error.transport_attempted = true;
    error
}

fn with_completed_steps(mut error: ExecutionError, completed_steps: usize) -> ExecutionError {
    error.completed_steps = completed_steps;
    error
}

fn invalid_target(message: impl Into<String>) -> ExecutionError {
    execution_error(ExecutionErrorCode::InvalidTarget, None, message)
}

fn step_error(
    code: ExecutionErrorCode,
    step: &PacketStep,
    message: impl Into<String>,
) -> ExecutionError {
    execution_error(code, Some(step.name), message)
}

fn execution_error(
    code: ExecutionErrorCode,
    step: Option<&'static str>,
    message: impl Into<String>,
) -> ExecutionError {
    ExecutionError {
        code,
        step,
        message: message.into(),
        completed_steps: 0,
        transport_attempted: false,
    }
}
