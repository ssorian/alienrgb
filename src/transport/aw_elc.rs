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
        let descriptor = device
            .device_descriptor()
            .map_err(|_| BackendError::new("aw_elc_descriptor_failed"))?;
        let handle = device
            .open()
            .map_err(|_| BackendError::new("aw_elc_open_failed"))?;
        let serial = read_bound_serial(&handle, &descriptor, selection)?;
        let interface = interface_evidence(&device)?;
        let before_claim = driver_state(&handle);
        let preclaim = AwOpenedEvidence {
            bus_number: device.bus_number(),
            port_path: device
                .port_numbers()
                .map_err(|_| BackendError::new("usb_port_path_failed"))?,
            vendor_id: descriptor.vendor_id(),
            product_id: descriptor.product_id(),
            serial,
            interface: interface.clone(),
            driver_before_claim: before_claim,
            driver_after_claim: DriverState::Inactive,
        };
        validate_aw_opened(selection, &preclaim)
            .map_err(|_| BackendError::new("aw_elc_preclaim_validation_failed"))?;
        handle
            .claim_interface(INTERFACE)
            .map_err(|_| BackendError::new("aw_elc_claim_failed"))?;
        let postclaim = AwOpenedEvidence {
            interface: interface_evidence(&device)?,
            driver_after_claim: driver_state(&handle),
            ..preclaim
        };
        validate_aw_opened(selection, &postclaim)
            .map_err(|_| BackendError::new("aw_elc_postclaim_validation_failed"))?;
        Ok(Box::new(RusbAwElcBackend { handle }))
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
    if evidence.interface.number != 0
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
    for state in [evidence.driver_before_claim, evidence.driver_after_claim] {
        match state {
            DriverState::Inactive => {}
            DriverState::Active => {
                return Err(execution_error(
                    ExecutionErrorCode::DriverActive,
                    None,
                    "AW-ELC kernel driver is active; it will not be detached",
                ))
            }
            DriverState::Unknown => {
                return Err(execution_error(
                    ExecutionErrorCode::DriverStateUnknown,
                    None,
                    "AW-ELC kernel-driver state is unavailable",
                ))
            }
        }
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

struct RusbAwElcBackend {
    handle: DeviceHandle<Context>,
}

impl AwElcBackend for RusbAwElcBackend {
    fn interrupt_write(&mut self, data: &[u8]) -> Result<usize, BackendError> {
        self.handle
            .write_interrupt(ENDPOINT_OUT, data, IO_TIMEOUT)
            .map_err(|_| BackendError::new("aw_elc_interrupt_write_failed"))
    }
}
