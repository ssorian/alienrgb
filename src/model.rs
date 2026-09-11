use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolFamily {
    AlienFxApiV4,
    AlienFxApiV5,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferKind {
    HidFeatureWrite,
    HidFeatureReadIntent,
    UsbOutput,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationState {
    MappingDerivedUnvalidated,
    ExactTouchpadStaticRedLiveValidated,
    ExactBackStaticRedLiveValidated,
    ExactPowerProfileStaticRedBatteryOnObserved,
    MixedChassisTargetEvidence,
    ExactSetAllHotPinkLiveValidated,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct RgbValue {
    pub(crate) r: u8,
    pub(crate) g: u8,
    pub(crate) b: u8,
}

impl RgbValue {
    pub fn r(self) -> u8 {
        self.r
    }
    pub fn g(self) -> u8 {
        self.g
    }
    pub fn b(self) -> u8 {
        self.b
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PlanAssignment {
    pub(crate) target: String,
    pub(crate) logical_id: u8,
    pub(crate) encoded_id: Option<u8>,
    pub(crate) color: RgbValue,
    pub(crate) color_hex: String,
    pub(crate) validation: ValidationState,
}

impl PlanAssignment {
    pub fn target(&self) -> &str {
        &self.target
    }
    pub fn logical_id(&self) -> u8 {
        self.logical_id
    }
    pub fn encoded_id(&self) -> Option<u8> {
        self.encoded_id
    }
    pub fn color(&self) -> RgbValue {
        self.color
    }
    pub fn color_hex(&self) -> &str {
        &self.color_hex
    }
    pub fn validation(&self) -> ValidationState {
        self.validation
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PacketStep {
    pub(crate) name: &'static str,
    pub(crate) transfer: TransferKind,
    pub(crate) caller_buffer_length: usize,
    pub(crate) on_wire_length: usize,
    pub(crate) payload_hex: Option<String>,
}

impl PacketStep {
    pub fn name(&self) -> &'static str {
        self.name
    }
    pub fn transfer(&self) -> TransferKind {
        self.transfer
    }
    pub fn caller_buffer_length(&self) -> usize {
        self.caller_buffer_length
    }
    pub fn on_wire_length(&self) -> usize {
        self.on_wire_length
    }
    pub fn payload_hex(&self) -> Option<&str> {
        self.payload_hex.as_deref()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PlanAssumption {
    pub(crate) name: &'static str,
    pub(crate) state: ValidationState,
    pub(crate) detail: &'static str,
}

impl PlanAssumption {
    pub fn name(&self) -> &'static str {
        self.name
    }
    pub fn state(&self) -> ValidationState {
        self.state
    }
    pub fn detail(&self) -> &'static str {
        self.detail
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProtocolPlan {
    pub(crate) family: ProtocolFamily,
    pub(crate) validation: ValidationState,
    pub(crate) assignments: Vec<PlanAssignment>,
    pub(crate) assumptions: Vec<PlanAssumption>,
    pub(crate) steps: Vec<PacketStep>,
}

impl ProtocolPlan {
    pub fn family(&self) -> ProtocolFamily {
        self.family
    }
    pub fn validation(&self) -> ValidationState {
        self.validation
    }
    pub fn assignments(&self) -> &[PlanAssignment] {
        &self.assignments
    }
    pub fn assumptions(&self) -> &[PlanAssumption] {
        &self.assumptions
    }
    pub fn steps(&self) -> &[PacketStep] {
        &self.steps
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandKind {
    List,
    Info,
    Doctor,
    Zones,
    Set,
    PowerProfile,
    KeyboardStatus,
    SetAll,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceKind {
    Keyboard,
    Chassis,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceStatus {
    Found,
    Missing,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DmiIdentity {
    pub vendor: Option<String>,
    pub product: Option<String>,
    pub bios_version: Option<String>,
    pub supported: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HidrawNodeState {
    Present,
    Missing,
    MetadataUnavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionEstimate {
    AllowedByModeBits,
    DeniedByModeBits,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectiveAccess {
    Allowed,
    Denied,
    Unknown,
    Missing,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct HidrawInfo {
    pub path: String,
    pub interface_number: Option<String>,
    pub node_state: HidrawNodeState,
    pub read_access: PermissionEstimate,
    pub write_access: PermissionEstimate,
    pub access_basis: &'static str,
    pub effective_read_access: EffectiveAccess,
    pub effective_write_access: EffectiveAccess,
    pub effective_access_basis: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DescriptorStatus {
    pub evidence: crate::profile::DescriptorEvidence,
    pub sha256: Option<String>,
    pub expected_sha256: &'static str,
    pub interface_number: Option<String>,
    pub sysfs_path: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DeviceSummary {
    pub kind: DeviceKind,
    pub status: DeviceStatus,
    pub vid: String,
    pub pid: String,
    pub manufacturer: Option<String>,
    pub product: Option<String>,
    pub serial: Option<String>,
    pub sysfs_name: Option<String>,
    pub bus_number: Option<u8>,
    pub port_path: Option<Vec<u8>>,
    pub interface_number: Option<String>,
    pub hidraw: Vec<HidrawInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub descriptor: Option<DescriptorStatus>,
}

impl DeviceSummary {
    pub fn missing(kind: DeviceKind) -> Self {
        let (vid, pid) = match kind {
            DeviceKind::Keyboard => ("0d62", "d2b1"),
            DeviceKind::Chassis => ("187c", "0551"),
        };
        Self {
            kind,
            status: DeviceStatus::Missing,
            vid: vid.into(),
            pid: pid.into(),
            manufacturer: None,
            product: None,
            serial: None,
            sysfs_name: None,
            bus_number: None,
            port_path: None,
            interface_number: None,
            hidraw: Vec::new(),
            descriptor: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ListReport {
    pub schema_version: u8,
    pub command: CommandKind,
    pub supported_system: bool,
    pub devices: Vec<DeviceSummary>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CapabilityProfile {
    pub kind: DeviceKind,
    pub name: &'static str,
    pub transport: &'static str,
    pub logical_targets: Vec<&'static str>,
    pub write_support: &'static str,
    pub validation_summary: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct InfoReport {
    pub schema_version: u8,
    pub command: CommandKind,
    pub dmi: DmiIdentity,
    pub capabilities: Vec<CapabilityProfile>,
    pub devices: Vec<DeviceSummary>,
    pub safety: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingLevel {
    Ok,
    Warning,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Finding {
    pub code: &'static str,
    pub level: FindingLevel,
    pub message: String,
    pub action: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DoctorReport {
    pub schema_version: u8,
    pub command: CommandKind,
    pub ready_for_diagnostics: bool,
    pub ready_for_writes: bool,
    pub keyboard_ready_for_live_write: bool,
    pub chassis_ready_for_live_write: bool,
    pub findings: Vec<Finding>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    DryRun,
    Apply,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationMode {
    StaticColor,
    PowerProfileEqualColor,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct KnownValidationRecord {
    pub mode: ValidationMode,
    pub color: &'static str,
    pub device_profile: &'static str,
    pub bios_version: &'static str,
    pub controller: &'static str,
    pub target: &'static str,
    pub logical_id: u8,
    pub evidence: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ZoneSummary {
    pub device: DeviceKind,
    pub usb_controller: &'static str,
    pub target: &'static str,
    pub logical_id: u8,
    pub aliases: &'static [&'static str],
    pub validation: ValidationState,
    pub validation_detail: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub known_validation: Option<KnownValidationRecord>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ZonesReport {
    pub schema_version: u8,
    pub command: CommandKind,
    pub usb_controller_count: u8,
    pub note: &'static str,
    pub zones: Vec<ZoneSummary>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DryRunReport {
    pub schema_version: u8,
    pub mode: ExecutionMode,
    pub transport_performed: bool,
    pub requested_device: DeviceKind,
    pub plan: ProtocolPlan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct PowerStateSummary {
    pub id: u8,
    pub name: &'static str,
    pub packet_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PowerProfileReport {
    pub schema_version: u8,
    pub command: CommandKind,
    pub mode: ExecutionMode,
    pub profile_kind: &'static str,
    pub power_profile: bool,
    pub transport_performed: bool,
    pub controller: &'static str,
    pub power_logical_id: u8,
    pub validation: ValidationState,
    pub color: String,
    pub color_applies_to: [&'static str; 2],
    pub states: Vec<PowerStateSummary>,
    pub packet_count: usize,
    pub plan: ProtocolPlan,
    pub executed_steps: Vec<LiveExecutedStep>,
    pub persistence: &'static str,
    pub state_restore_available: bool,
    pub readback_available: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct LiveExecutedStep {
    pub name: &'static str,
    pub transferred_length: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct KeyboardStatusReport {
    pub schema_version: u8,
    pub command: CommandKind,
    pub transport_performed: bool,
    pub color_frames_sent: bool,
    pub query_write_length: usize,
    pub response_length: usize,
    pub response_hex: String,
    pub persistence: &'static str,
    pub color_readback: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SetAllPlanStage {
    pub assignment_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color_frame_count: Option<usize>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub logical_ids: Vec<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state_count: Option<usize>,
    pub step_count: usize,
    pub plan: ProtocolPlan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompoundStatus {
    Completed,
    PartialFailure,
    PreflightFailure,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompoundStageStatus {
    Completed,
    Failed,
    NotStarted,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CompoundFailure {
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<String>,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CompoundStageRecord {
    pub stage: &'static str,
    pub status: CompoundStageStatus,
    pub expected_steps: usize,
    pub completed_steps: usize,
    pub transport_attempted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<CompoundFailure>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SetAllReport {
    pub schema_version: u8,
    pub command: CommandKind,
    pub mode: ExecutionMode,
    pub color: String,
    pub validation: ValidationState,
    pub validation_evidence: String,
    pub transport_attempted: bool,
    pub transport_performed: bool,
    pub keyboard: SetAllPlanStage,
    pub aw_static_touchpad_back: SetAllPlanStage,
    pub power_profile: SetAllPlanStage,
    pub total_transport_steps: usize,
    pub safety_warning: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overall_status: Option<CompoundStatus>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub stages: Vec<CompoundStageRecord>,
    pub rollback: &'static str,
    pub persistence: &'static str,
    pub readback: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct LiveWriteReport {
    pub schema_version: u8,
    pub command: CommandKind,
    pub mode: ExecutionMode,
    pub transport_performed: bool,
    pub requested_device: DeviceKind,
    pub requested_target: String,
    pub resolved_target: String,
    pub canonical_targets: Vec<String>,
    pub target_count: usize,
    pub set_color_frames: usize,
    pub color: String,
    pub controller_vid: &'static str,
    pub controller_pid: &'static str,
    pub executed_steps: Vec<LiveExecutedStep>,
    pub state_restore_available: bool,
    pub persistence: &'static str,
}
