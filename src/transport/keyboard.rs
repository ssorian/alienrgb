use super::{
    execution_error, BackendError, ExecutionError, ExecutionErrorCode, KeyboardBackend,
    KeyboardBackendFactory, KeyboardSelection,
};
use crate::profile::KEYBOARD_DESCRIPTOR_SHA256;
use hidapi::{BusType, HidApi, HidDevice, HidError, MAX_REPORT_DESCRIPTOR_SIZE};
use sha2::{Digest, Sha256};
use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct KeyboardEnumerationRecord {
    pub(super) path: Vec<u8>,
    pub(super) is_usb: bool,
    pub(super) vendor_id: u16,
    pub(super) product_id: u16,
    pub(super) interface_number: i32,
    pub(super) manufacturer: Option<String>,
    pub(super) product: Option<String>,
    pub(super) usage_page: u16,
    pub(super) usage: u16,
}

pub(super) fn validate_keyboard_enumeration(
    selected_path: &[u8],
    records: &[KeyboardEnumerationRecord],
) -> Result<usize, BackendError> {
    let same_path = records
        .iter()
        .enumerate()
        .filter(|(_, record)| record.path == selected_path)
        .collect::<Vec<_>>();
    if same_path.is_empty() {
        return Err(BackendError::new("keyboard_exact_path_missing"));
    }

    let mut manufacturer = None;
    let mut product = None;
    let mut rgb_record = None;
    for (index, record) in same_path {
        if !record.is_usb
            || record.vendor_id != 0x0d62
            || record.product_id != 0xd2b1
            || record.interface_number != 0
        {
            return Err(BackendError::new("keyboard_same_path_identity_conflict"));
        }
        require_consistent_optional(
            &mut manufacturer,
            record.manufacturer.as_deref(),
            "keyboard_same_path_manufacturer_conflict",
        )?;
        require_consistent_optional(
            &mut product,
            record.product.as_deref(),
            "keyboard_same_path_product_conflict",
        )?;
        if record.usage_page == 0xff89
            && record.usage == 0x00cc
            && rgb_record.replace(index).is_some()
        {
            return Err(BackendError::new("keyboard_rgb_collection_duplicate"));
        }
    }
    rgb_record.ok_or_else(|| BackendError::new("keyboard_rgb_collection_missing"))
}

fn require_consistent_optional<'a>(
    expected: &mut Option<&'a str>,
    actual: Option<&'a str>,
    error_code: &'static str,
) -> Result<(), BackendError> {
    if let Some(actual) = actual {
        if expected.is_some_and(|expected| expected != actual) {
            return Err(BackendError::new(error_code));
        }
        *expected = Some(actual);
    }
    Ok(())
}

pub(crate) struct HidApiKeyboardFactory;

impl Default for HidApiKeyboardFactory {
    fn default() -> Self {
        Self
    }
}

impl KeyboardBackendFactory for HidApiKeyboardFactory {
    fn requires_process_guard(&self) -> bool {
        true
    }

    fn acquire(
        &mut self,
        selection: &KeyboardSelection,
    ) -> Result<Box<dyn KeyboardBackend>, BackendError> {
        let path = CString::new(selection.path.as_os_str().as_bytes())
            .map_err(|_| BackendError::new("keyboard_path_contains_nul"))?;
        let api = HidApi::new().map_err(|_| BackendError::new("keyboard_enumeration_failed"))?;
        let records = api
            .device_list()
            .map(|device| KeyboardEnumerationRecord {
                path: device.path().to_bytes().to_vec(),
                is_usb: matches!(device.bus_type(), BusType::Usb),
                vendor_id: device.vendor_id(),
                product_id: device.product_id(),
                interface_number: device.interface_number(),
                manufacturer: device.manufacturer_string().map(str::to_owned),
                product: device.product_string().map(str::to_owned),
                usage_page: device.usage_page(),
                usage: device.usage(),
            })
            .collect::<Vec<_>>();
        let rgb_record = validate_keyboard_enumeration(path.as_bytes(), &records)?;
        let selected_path = CString::new(records[rgb_record].path.clone())
            .map_err(|_| BackendError::new("keyboard_enumerated_path_contains_nul"))?;

        let device = api
            .open_path(selected_path.as_c_str())
            .map_err(map_open_error)?;
        let info = device
            .get_device_info()
            .map_err(|_| BackendError::new("keyboard_opened_identity_failed"))?;
        let mut descriptor = vec![0_u8; MAX_REPORT_DESCRIPTOR_SIZE];
        let descriptor_length = device
            .get_report_descriptor(&mut descriptor)
            .map_err(|_| BackendError::new("keyboard_opened_descriptor_failed"))?;
        descriptor.truncate(descriptor_length);
        let evidence = OpenedKeyboardEvidence {
            vendor_id: info.vendor_id(),
            product_id: info.product_id(),
            interface_number: info.interface_number(),
            path: PathBuf::from(std::ffi::OsStr::from_bytes(info.path().to_bytes())),
            report_descriptor_sha256: format!("{:x}", Sha256::digest(&descriptor)),
        };
        validate_opened_keyboard(selection, &evidence)
            .map_err(|_| BackendError::new("keyboard_opened_identity_drift"))?;
        Ok(Box::new(HidApiKeyboardBackend { device }))
    }
}

fn map_open_error(error: HidError) -> BackendError {
    if matches!(
        &error,
        HidError::IoError { error } if error.kind() == std::io::ErrorKind::PermissionDenied
    ) {
        BackendError::new("keyboard_permission_denied")
    } else {
        BackendError::new("keyboard_open_failed")
    }
}

struct HidApiKeyboardBackend {
    device: HidDevice,
}

impl KeyboardBackend for HidApiKeyboardBackend {
    fn send_feature_report(&mut self, data: &[u8]) -> Result<usize, BackendError> {
        self.device
            .send_feature_report(data)
            .map(|()| data.len())
            .map_err(|_| BackendError::new("keyboard_feature_write_failed"))
    }

    fn get_feature_report(&mut self, data: &mut [u8]) -> Result<usize, BackendError> {
        self.device
            .get_feature_report(data)
            .map_err(|_| BackendError::new("keyboard_feature_read_failed"))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct OpenedKeyboardEvidence {
    pub(super) vendor_id: u16,
    pub(super) product_id: u16,
    pub(super) interface_number: i32,
    pub(super) path: PathBuf,
    pub(super) report_descriptor_sha256: String,
}

pub(super) fn validate_opened_keyboard(
    selection: &KeyboardSelection,
    evidence: &OpenedKeyboardEvidence,
) -> Result<(), ExecutionError> {
    if evidence.vendor_id != 0x0d62
        || evidence.product_id != 0xd2b1
        || evidence.path != selection.path
    {
        return Err(execution_error(
            ExecutionErrorCode::IdentityDrift,
            None,
            "opened keyboard VID/PID/path identity differs from fresh discovery",
        ));
    }
    if evidence.interface_number != 0 {
        return Err(execution_error(
            ExecutionErrorCode::InterfaceDrift,
            None,
            "opened keyboard interface is not interface 00",
        ));
    }
    if evidence.report_descriptor_sha256 != KEYBOARD_DESCRIPTOR_SHA256 {
        return Err(execution_error(
            ExecutionErrorCode::IdentityDrift,
            None,
            "opened keyboard report descriptor hash does not match confirmed hardware",
        ));
    }
    Ok(())
}
