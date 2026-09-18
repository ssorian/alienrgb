use super::{
    execution_error, AwElcBackend, AwElcBackendFactory, AwElcSelection, BackendError,
    ExecutionError, ExecutionErrorCode,
};
use rusb::{Context, Device, DeviceHandle, Direction, TransferType, UsbContext};
use std::time::Duration;

const INTERFACE: u8 = 0;
const ENDPOINT_OUT: u8 = 0x01;
const ENDPOINT_IN: u8 = 0x81;
const MAX_PACKET_SIZE: u16 = 33;
const IO_TIMEOUT: Duration = Duration::from_millis(500);

pub(crate) struct RusbAwElcFactory;

impl AwElcBackendFactory for RusbAwElcFactory {
    fn requires_process_guard(&self) -> bool {
        true
    }

    fn acquire(
        &mut self,
        selection: &AwElcSelection,
    ) -> Result<Box<dyn AwElcBackend>, BackendError> {
        let context = Context::new().map_err(|_| BackendError::new("usb_initialization_failed"))?;
        let devices = context
            .devices()
            .map_err(|_| BackendError::new("usb_enumeration_failed"))?;
        let mut matching = Vec::new();
        for device in devices.iter() {
            let port_path = device
                .port_numbers()
                .map_err(|_| BackendError::new("usb_port_path_failed"))?;
            if device.bus_number() == selection.bus_number && port_path == selection.port_path {
                matching.push(device);
            }
        }
        if matching.len() != 1 {
            return Err(BackendError::new("aw_elc_bound_device_count_mismatch"));
        }
        let device = matching.pop().expect("one bus/port-bound device");
        let handle = device
            .open()
            .map_err(|_| BackendError::new("aw_elc_open_failed"))?;
        Ok(Box::new(acquire_aw_elc_handoff(
            selection,
            RusbAwElcHandoff { device, handle },
        )?))
    }
}

pub(super) trait AwElcHandoff {
    fn evidence(&mut self, selection: &AwElcSelection) -> Result<AwOpenedEvidence, BackendError>;
    fn driver_state(&mut self) -> DriverState;
    fn detach_kernel_driver(&mut self) -> Result<(), BackendError>;
    fn claim_interface(&mut self) -> Result<(), BackendError>;
    fn release_interface(&mut self) -> Result<(), BackendError>;
    fn attach_kernel_driver(&mut self) -> Result<(), BackendError>;
    fn interrupt_write(&mut self, data: &[u8]) -> Result<usize, BackendError>;
}

pub(super) fn acquire_aw_elc_handoff<H: AwElcHandoff + 'static>(
    selection: &AwElcSelection,
    mut handle: H,
) -> Result<HandoffAwElcBackend<H>, BackendError> {
    let mut preclaim = handle.evidence(selection)?;
    preclaim.driver_before_claim = handle.driver_state();
    validate_aw_preclaim(selection, &preclaim)
        .map_err(|_| BackendError::new("aw_elc_preclaim_validation_failed"))?;

    let mut backend = HandoffAwElcBackend {
        handle,
        detached: false,
        claimed: false,
        finished: false,
    };
    if preclaim.driver_before_claim == DriverState::Active {
        if let Err(detach) = backend.handle.detach_kernel_driver() {
            return Err(backend.detach_failure(detach));
        }
        backend.detached = true;
    }
    if backend.handle.claim_interface().is_err() {
        return Err(backend.acquire_failure(BackendError::new("aw_elc_claim_failed")));
    }
    backend.claimed = true;

    let mut postclaim = match backend.handle.evidence(selection) {
        Ok(evidence) => evidence,
        Err(error) => return Err(backend.acquire_failure(error)),
    };
    postclaim.driver_after_claim = backend.handle.driver_state();
    if validate_aw_postclaim(selection, &postclaim).is_err() {
        return Err(
            backend.acquire_failure(BackendError::new("aw_elc_postclaim_validation_failed"))
        );
    }
    Ok(backend)
}

struct RusbAwElcHandoff {
    device: Device<Context>,
    handle: DeviceHandle<Context>,
}

impl AwElcHandoff for RusbAwElcHandoff {
    fn evidence(&mut self, selection: &AwElcSelection) -> Result<AwOpenedEvidence, BackendError> {
        let descriptor = self
            .device
            .device_descriptor()
            .map_err(|_| BackendError::new("aw_elc_descriptor_failed"))?;
        let serial = read_bound_serial(&self.handle, &descriptor, selection)?;
        Ok(AwOpenedEvidence {
            bus_number: self.device.bus_number(),
            port_path: self
                .device
                .port_numbers()
                .map_err(|_| BackendError::new("usb_port_path_failed"))?,
            vendor_id: descriptor.vendor_id(),
            product_id: descriptor.product_id(),
            serial,
            interface: interface_evidence(&self.device)?,
            driver_before_claim: DriverState::Inactive,
            driver_after_claim: DriverState::Inactive,
        })
    }

    fn driver_state(&mut self) -> DriverState {
        driver_state(&self.handle)
    }

    fn detach_kernel_driver(&mut self) -> Result<(), BackendError> {
        self.handle
            .detach_kernel_driver(INTERFACE)
            .map_err(|_| BackendError::new("aw_elc_detach_failed"))
    }

    fn claim_interface(&mut self) -> Result<(), BackendError> {
        self.handle
            .claim_interface(INTERFACE)
            .map_err(|_| BackendError::new("aw_elc_claim_failed"))
    }

    fn release_interface(&mut self) -> Result<(), BackendError> {
        self.handle
            .release_interface(INTERFACE)
            .map_err(|_| BackendError::new("aw_elc_release_failed"))
    }

    fn attach_kernel_driver(&mut self) -> Result<(), BackendError> {
        self.handle
            .attach_kernel_driver(INTERFACE)
            .map_err(|_| BackendError::new("aw_elc_reattach_failed"))
    }

    fn interrupt_write(&mut self, data: &[u8]) -> Result<usize, BackendError> {
        self.handle
            .write_interrupt(ENDPOINT_OUT, data, IO_TIMEOUT)
            .map_err(|_| BackendError::new("aw_elc_interrupt_write_failed"))
    }
}

fn read_bound_serial(
    handle: &DeviceHandle<Context>,
    descriptor: &rusb::DeviceDescriptor,
    selection: &AwElcSelection,
) -> Result<Option<String>, BackendError> {
    let Some(expected) = selection.serial.as_ref() else {
        return Ok(None);
    };
    let index = descriptor
        .serial_number_string_index()
        .ok_or_else(|| BackendError::new("aw_elc_serial_missing"))?;
    let languages = handle
        .read_languages(IO_TIMEOUT)
        .map_err(|_| BackendError::new("aw_elc_serial_language_failed"))?;
    let language = languages
        .first()
        .ok_or_else(|| BackendError::new("aw_elc_serial_language_missing"))?;
    let actual = handle
        .read_string_descriptor(*language, index, IO_TIMEOUT)
        .map_err(|_| BackendError::new("aw_elc_serial_read_failed"))?;
    if &actual != expected {
        return Err(BackendError::new("aw_elc_serial_mismatch"));
    }
    Ok(Some(actual))
}

fn interface_evidence(device: &Device<Context>) -> Result<AwInterfaceEvidence, BackendError> {
    let config = device
        .active_config_descriptor()
        .map_err(|_| BackendError::new("aw_elc_config_descriptor_failed"))?;
    let interfaces = config
        .interfaces()
        .filter(|interface| interface.number() == INTERFACE)
        .collect::<Vec<_>>();
    if interfaces.len() != 1 {
        return Err(BackendError::new("aw_elc_interface_mismatch"));
    }
    let descriptors = interfaces[0].descriptors().collect::<Vec<_>>();
    if descriptors.len() != 1 {
        return Err(BackendError::new("aw_elc_interface_mismatch"));
    }
    let descriptor = &descriptors[0];
    Ok(AwInterfaceEvidence {
        number: descriptor.interface_number(),
        class: descriptor.class_code(),
        subclass: descriptor.sub_class_code(),
        protocol: descriptor.protocol_code(),
        endpoints: descriptor
            .endpoint_descriptors()
            .map(|endpoint| EndpointEvidence {
                address: endpoint.address(),
                direction: endpoint.direction(),
                transfer_type: endpoint.transfer_type(),
                max_packet_size: endpoint.max_packet_size(),
            })
            .collect(),
    })
}

fn driver_state(handle: &DeviceHandle<Context>) -> DriverState {
    match handle.kernel_driver_active(INTERFACE) {
        Ok(true) => DriverState::Active,
        Ok(false) => DriverState::Inactive,
        Err(_) => DriverState::Unknown,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DriverState {
    Active,
    Inactive,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct EndpointEvidence {
    pub(super) address: u8,
    pub(super) direction: Direction,
    pub(super) transfer_type: TransferType,
    pub(super) max_packet_size: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AwInterfaceEvidence {
    pub(super) number: u8,
    pub(super) class: u8,
    pub(super) subclass: u8,
    pub(super) protocol: u8,
    pub(super) endpoints: Vec<EndpointEvidence>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AwOpenedEvidence {
    pub(super) bus_number: u8,
    pub(super) port_path: Vec<u8>,
    pub(super) vendor_id: u16,
    pub(super) product_id: u16,
    pub(super) serial: Option<String>,
    pub(super) interface: AwInterfaceEvidence,
    pub(super) driver_before_claim: DriverState,
    pub(super) driver_after_claim: DriverState,
}

pub(super) fn validate_aw_opened(
    selection: &AwElcSelection,
    evidence: &AwOpenedEvidence,
) -> Result<(), ExecutionError> {
    validate_aw_preclaim(selection, evidence)?;
    validate_aw_postclaim(selection, evidence)
}

fn validate_aw_preclaim(
    selection: &AwElcSelection,
    evidence: &AwOpenedEvidence,
) -> Result<(), ExecutionError> {
    validate_aw_identity_and_interface(selection, evidence)?;
    if evidence.driver_before_claim == DriverState::Unknown {
        return Err(execution_error(
            ExecutionErrorCode::DriverStateUnknown,
            None,
            "AW-ELC kernel-driver state is unavailable before handoff",
        ));
    }
    Ok(())
}

fn validate_aw_postclaim(
    selection: &AwElcSelection,
    evidence: &AwOpenedEvidence,
) -> Result<(), ExecutionError> {
    validate_aw_identity_and_interface(selection, evidence)?;
    match evidence.driver_after_claim {
        DriverState::Inactive => Ok(()),
        DriverState::Active => Err(execution_error(
            ExecutionErrorCode::DriverActive,
            None,
            "AW-ELC kernel driver remains active after interface-0 claim",
        )),
        DriverState::Unknown => Err(execution_error(
            ExecutionErrorCode::DriverStateUnknown,
            None,
            "AW-ELC kernel-driver state is unavailable after interface-0 claim",
        )),
    }
}

fn validate_aw_identity_and_interface(
    selection: &AwElcSelection,
    evidence: &AwOpenedEvidence,
) -> Result<(), ExecutionError> {
    if evidence.bus_number != selection.bus_number
        || evidence.port_path != selection.port_path
        || evidence.vendor_id != 0x187c
        || evidence.product_id != 0x0551
        || evidence.serial != selection.serial
    {
        return Err(execution_error(
            ExecutionErrorCode::IdentityDrift,
            None,
            "opened AW-ELC identity differs from fresh bus/port/serial discovery",
        ));
    }
    if evidence.interface.number != INTERFACE
        || evidence.interface.class != 3
        || evidence.interface.subclass != 0
        || evidence.interface.protocol != 0
    {
        return Err(execution_error(
            ExecutionErrorCode::InterfaceDrift,
            None,
            "opened AW-ELC interface descriptor drifted",
        ));
    }
    if evidence.interface.endpoints.len() != 2
        || !has_endpoint(&evidence.interface, ENDPOINT_OUT, Direction::Out)
        || !has_endpoint(&evidence.interface, ENDPOINT_IN, Direction::In)
    {
        return Err(execution_error(
            ExecutionErrorCode::EndpointDrift,
            None,
            "opened AW-ELC endpoint descriptors drifted",
        ));
    }
    Ok(())
}

fn has_endpoint(interface: &AwInterfaceEvidence, address: u8, direction: Direction) -> bool {
    interface.endpoints.iter().any(|endpoint| {
        endpoint.address == address
            && endpoint.direction == direction
            && endpoint.transfer_type == TransferType::Interrupt
            && endpoint.max_packet_size == MAX_PACKET_SIZE
    })
}

pub(super) struct HandoffAwElcBackend<H: AwElcHandoff> {
    handle: H,
    detached: bool,
    claimed: bool,
    finished: bool,
}

impl<H: AwElcHandoff> HandoffAwElcBackend<H> {
    fn detach_failure(&mut self, detach: BackendError) -> BackendError {
        match self.handle.driver_state() {
            DriverState::Inactive => {
                self.detached = true;
                let primary = BackendError::new(format!(
                    "{}; driver inactive after detach error; attempting attach cleanup",
                    detach.code
                ));
                match self.close() {
                    Ok(()) => {
                        BackendError::new(format!("{}; attach cleanup succeeded", primary.code))
                    }
                    Err(cleanup) => BackendError::combined(primary, cleanup),
                }
            }
            DriverState::Active => BackendError::new(format!(
                "{}; driver remains active after detach error; no attach attempted",
                detach.code
            )),
            DriverState::Unknown => BackendError::new(format!(
                "{}; driver state is unknown after detach error; no attach attempted",
                detach.code
            )),
        }
    }

    fn acquire_failure(&mut self, primary: BackendError) -> BackendError {
        match self.close() {
            Ok(()) => primary,
            Err(cleanup) => BackendError::combined(primary, cleanup),
        }
    }

    fn close(&mut self) -> Result<(), BackendError> {
        if self.finished {
            return Ok(());
        }
        self.finished = true;
        let mut failures = Vec::new();
        if self.claimed {
            self.claimed = false;
            if let Err(error) = self.handle.release_interface() {
                failures.push(error);
            }
        }
        if self.detached {
            self.detached = false;
            if let Err(error) = self.handle.attach_kernel_driver() {
                failures.push(error);
            }
        }
        BackendError::from_cleanup_failures(failures)
    }
}

impl<H: AwElcHandoff> AwElcBackend for HandoffAwElcBackend<H> {
    fn interrupt_write(&mut self, data: &[u8]) -> Result<usize, BackendError> {
        self.handle.interrupt_write(data)
    }

    fn finish(&mut self) -> Result<(), BackendError> {
        self.close()
    }
}

impl<H: AwElcHandoff> Drop for HandoffAwElcBackend<H> {
    fn drop(&mut self) {
        let _ = self.close();
    }
}
