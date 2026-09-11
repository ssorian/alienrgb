use crate::model::{
    DescriptorStatus, DeviceKind, DeviceStatus, DeviceSummary, DmiIdentity, EffectiveAccess,
    HidrawInfo, HidrawNodeState, PermissionEstimate,
};
use crate::profile::{
    descriptor_evidence, match_system, DescriptorEvidence, CHASSIS_PID, CHASSIS_VID,
    KEYBOARD_DESCRIPTOR_SHA256, KEYBOARD_PID, KEYBOARD_VID,
};
use rustix::fs::{accessat, Access, AtFlags, CWD};
use rustix::io::Errno;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

pub const DEFAULT_SYS_ROOT: &str = "/sys";

pub fn discover() -> io::Result<(DmiIdentity, Vec<DeviceSummary>)> {
    discover_internal(
        Path::new(DEFAULT_SYS_ROOT),
        Path::new("/dev"),
        HidrawMode::Metadata,
    )
}

pub fn discover_for_planning() -> io::Result<(DmiIdentity, Vec<DeviceSummary>)> {
    discover_internal(
        Path::new(DEFAULT_SYS_ROOT),
        Path::new("/dev"),
        HidrawMode::None,
    )
}

#[allow(dead_code)]
pub(crate) fn discover_for_transport() -> io::Result<(DmiIdentity, Vec<DeviceSummary>)> {
    discover_internal(
        Path::new(DEFAULT_SYS_ROOT),
        Path::new("/dev"),
        HidrawMode::PathsOnly,
    )
}

pub fn discover_at(
    sys_root: &Path,
    dev_root: &Path,
) -> io::Result<(DmiIdentity, Vec<DeviceSummary>)> {
    discover_internal(sys_root, dev_root, HidrawMode::Metadata)
}

#[derive(Clone, Copy)]
#[allow(dead_code)]
enum HidrawMode {
    None,
    PathsOnly,
    Metadata,
}

fn discover_internal(
    sys_root: &Path,
    dev_root: &Path,
    hidraw_mode: HidrawMode,
) -> io::Result<(DmiIdentity, Vec<DeviceSummary>)> {
    let dmi_root = sys_root.join("class/dmi/id");
    let vendor = read_trimmed(dmi_root.join("sys_vendor"));
    let product = read_trimmed(dmi_root.join("product_name"));
    let bios_version = read_trimmed(dmi_root.join("bios_version"));
    let supported = matches!((&vendor, &product), (Some(v), Some(p)) if match_system(v, p));
    let dmi = DmiIdentity {
        vendor,
        product,
        bios_version,
        supported,
    };

    if !supported {
        return Ok((dmi, Vec::new()));
    }

    let usb_root = sys_root.join("bus/usb/devices");
    let mut devices = Vec::new();
    let entries = match fs::read_dir(&usb_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok((dmi, devices)),
        Err(error) => return Err(error),
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let vid = read_trimmed(path.join("idVendor")).map(|v| v.to_ascii_lowercase());
        let pid = read_trimmed(path.join("idProduct")).map(|v| v.to_ascii_lowercase());
        let kind = match (vid.as_deref(), pid.as_deref()) {
            (Some(KEYBOARD_VID), Some(KEYBOARD_PID)) => Some(DeviceKind::Keyboard),
            (Some(CHASSIS_VID), Some(CHASSIS_PID)) => Some(DeviceKind::Chassis),
            _ => None,
        };
        if let Some(kind) = kind {
            devices.push(device_from_sysfs(
                kind,
                &path,
                &usb_root,
                dev_root,
                hidraw_mode,
            ));
        }
    }
    devices.sort_by(|left, right| {
        device_kind_rank(left.kind)
            .cmp(&device_kind_rank(right.kind))
            .then_with(|| left.sysfs_name.cmp(&right.sysfs_name))
            .then_with(|| left.serial.cmp(&right.serial))
    });
    Ok((dmi, devices))
}

fn device_kind_rank(kind: DeviceKind) -> u8 {
    match kind {
        DeviceKind::Keyboard => 0,
        DeviceKind::Chassis => 1,
    }
}

fn device_from_sysfs(
    kind: DeviceKind,
    path: &Path,
    usb_root: &Path,
    dev_root: &Path,
    hidraw_mode: HidrawMode,
) -> DeviceSummary {
    let (vid, pid) = match kind {
        DeviceKind::Keyboard => (KEYBOARD_VID, KEYBOARD_PID),
        DeviceKind::Chassis => (CHASSIS_VID, CHASSIS_PID),
    };
    let sysfs_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned());
    let selected_root = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let interfaces = interface_paths(
        usb_root,
        sysfs_name.as_deref().unwrap_or_default(),
        &selected_root,
    );
    let interface_number = interfaces
        .iter()
        .find_map(|interface| interface.number.clone());
    let hidraw_paths = interfaces
        .iter()
        .flat_map(|interface| {
            find_named_entries(&interface.path, "hidraw", 4)
                .into_iter()
                .filter_map(|entry| entry.file_name().map(|name| name.to_owned()))
                .filter(|name| {
                    name.to_string_lossy()
                        .strip_prefix("hidraw")
                        .is_some_and(|suffix| {
                            !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit())
                        })
                })
                .map(|name| (dev_root.join(name), interface.number.clone()))
        })
        .collect::<HashSet<_>>();
    let mut hidraw = match hidraw_mode {
        HidrawMode::Metadata => hidraw_paths
            .into_iter()
            .map(|(path, interface)| hidraw_info(&path, interface))
            .collect::<Vec<_>>(),
        HidrawMode::PathsOnly => hidraw_paths
            .into_iter()
            .map(|(path, interface_number)| HidrawInfo {
                path: path.display().to_string(),
                interface_number,
                node_state: HidrawNodeState::MetadataUnavailable,
                read_access: PermissionEstimate::Unknown,
                write_access: PermissionEstimate::Unknown,
                access_basis: "not_inspected",
                effective_read_access: EffectiveAccess::Unknown,
                effective_write_access: EffectiveAccess::Unknown,
                effective_access_basis: "not_inspected",
            })
            .collect::<Vec<_>>(),
        HidrawMode::None => Vec::new(),
    };
    hidraw.sort_by(|left, right| left.path.cmp(&right.path));

    let descriptor = if kind == DeviceKind::Keyboard {
        Some(keyboard_descriptor_status(&interfaces))
    } else {
        None
    };

    DeviceSummary {
        kind,
        status: DeviceStatus::Found,
        vid: vid.into(),
        pid: pid.into(),
        manufacturer: read_trimmed(path.join("manufacturer")),
        product: read_trimmed(path.join("product")),
        serial: read_trimmed(path.join("serial")),
        bus_number: read_trimmed(path.join("busnum")).and_then(|value| value.parse().ok()),
        port_path: sysfs_name.as_deref().and_then(parse_port_path),
        sysfs_name,
        interface_number,
        hidraw,
        descriptor,
    }
}

struct InterfacePath {
    path: PathBuf,
    number: Option<String>,
}

fn interface_paths(usb_root: &Path, device_name: &str, selected_root: &Path) -> Vec<InterfacePath> {
    let prefix = format!("{device_name}:");
    let mut paths = fs::read_dir(usb_root)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(&prefix))
        .filter_map(|entry| entry.path().canonicalize().ok())
        .filter(|path| path.starts_with(selected_root))
        .map(|path| InterfacePath {
            number: read_trimmed(path.join("bInterfaceNumber")),
            path,
        })
        .collect::<Vec<_>>();
    paths.sort_by(|left, right| {
        left.number
            .cmp(&right.number)
            .then_with(|| left.path.cmp(&right.path))
    });
    paths
}

fn keyboard_descriptor_status(interfaces: &[InterfacePath]) -> DescriptorStatus {
    let mut candidates = interfaces
        .iter()
        .flat_map(|interface| {
            find_named_entries(&interface.path, "report_descriptor", 4)
                .into_iter()
                .map(|path| (path, interface.number.clone()))
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| left.0.cmp(&right.0));

    let mut selected = None;
    for (path, interface_number) in candidates {
        let (evidence, sha256) = match fs::read(&path) {
            Ok(bytes) => {
                let hash = format!("{:x}", Sha256::digest(&bytes));
                (descriptor_evidence(&bytes, Some(&hash)), Some(hash))
            }
            Err(_) => (DescriptorEvidence::Unreadable, None),
        };
        let candidate = DescriptorStatus {
            evidence,
            sha256,
            expected_sha256: KEYBOARD_DESCRIPTOR_SHA256,
            interface_number,
            sysfs_path: Some(path.display().to_string()),
        };
        if selected.as_ref().is_none_or(|current: &DescriptorStatus| {
            descriptor_rank(candidate.evidence) > descriptor_rank(current.evidence)
        }) {
            selected = Some(candidate);
        }
    }

    selected.unwrap_or(DescriptorStatus {
        evidence: DescriptorEvidence::NotFound,
        sha256: None,
        expected_sha256: KEYBOARD_DESCRIPTOR_SHA256,
        interface_number: None,
        sysfs_path: None,
    })
}

fn descriptor_rank(evidence: DescriptorEvidence) -> u8 {
    match evidence {
        DescriptorEvidence::NotFound => 0,
        DescriptorEvidence::Unreadable => 1,
        DescriptorEvidence::Mismatch => 2,
        DescriptorEvidence::CompatibleSignature => 3,
        DescriptorEvidence::HashMatch => 4,
    }
}

fn find_named_entries(root: &Path, name_prefix: &str, depth: usize) -> Vec<PathBuf> {
    if depth == 0 {
        return Vec::new();
    }
    let mut found = Vec::new();
    let Ok(entries) = fs::read_dir(root) else {
        return found;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        if entry.file_name().to_string_lossy().starts_with(name_prefix) {
            found.push(path.clone());
        }
        if file_type.is_dir() {
            found.extend(find_named_entries(&path, name_prefix, depth - 1));
        }
    }
    found.sort();
    found
}

fn hidraw_info(path: &Path, interface_number: Option<String>) -> HidrawInfo {
    let (node_state, read_access, write_access, access_basis) = match fs::metadata(path) {
        Ok(metadata) => {
            let identity = process_identity();
            (
                HidrawNodeState::Present,
                estimate_permission(&metadata, identity.as_ref(), 0o4),
                estimate_permission(&metadata, identity.as_ref(), 0o2),
                "unix_mode_bits_estimate",
            )
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => (
            HidrawNodeState::Missing,
            PermissionEstimate::Unknown,
            PermissionEstimate::Unknown,
            "node_missing",
        ),
        Err(_) => (
            HidrawNodeState::MetadataUnavailable,
            PermissionEstimate::Unknown,
            PermissionEstimate::Unknown,
            "metadata_unavailable",
        ),
    };
    let (effective_read_access, effective_write_access) = if node_state == HidrawNodeState::Missing
    {
        (EffectiveAccess::Missing, EffectiveAccess::Missing)
    } else {
        (
            effective_access(path, Access::READ_OK),
            effective_access(path, Access::WRITE_OK),
        )
    };
    HidrawInfo {
        path: path.display().to_string(),
        interface_number,
        node_state,
        read_access,
        write_access,
        access_basis,
        effective_read_access,
        effective_write_access,
        effective_access_basis: "faccessat_at_eaccess",
    }
}

fn effective_access(path: &Path, access: Access) -> EffectiveAccess {
    match accessat(CWD, path, access, AtFlags::EACCESS) {
        Ok(()) => EffectiveAccess::Allowed,
        Err(Errno::NOENT) => EffectiveAccess::Missing,
        Err(Errno::ACCESS | Errno::PERM) => EffectiveAccess::Denied,
        Err(_) => EffectiveAccess::Unknown,
    }
}

fn estimate_permission(
    metadata: &fs::Metadata,
    identity: Option<&ProcessIdentity>,
    bit: u32,
) -> PermissionEstimate {
    match identity {
        Some(identity) if permits(metadata, identity, bit) => PermissionEstimate::AllowedByModeBits,
        Some(_) => PermissionEstimate::DeniedByModeBits,
        None => PermissionEstimate::Unknown,
    }
}

struct ProcessIdentity {
    uid: u32,
    groups: HashSet<u32>,
}

fn process_identity() -> Option<ProcessIdentity> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    let uid = status
        .lines()
        .find(|line| line.starts_with("Uid:"))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()?;
    let groups = status
        .lines()
        .find(|line| line.starts_with("Groups:"))
        .map(|line| {
            line.split_whitespace()
                .skip(1)
                .filter_map(|v| v.parse().ok())
                .collect()
        })
        .unwrap_or_default();
    Some(ProcessIdentity { uid, groups })
}

fn permits(metadata: &fs::Metadata, identity: &ProcessIdentity, bit: u32) -> bool {
    if identity.uid == 0 {
        return true;
    }
    let mode = metadata.mode();
    if identity.uid == metadata.uid() {
        mode & (bit << 6) != 0
    } else if identity.groups.contains(&metadata.gid()) {
        mode & (bit << 3) != 0
    } else {
        mode & bit != 0
    }
}

fn parse_port_path(sysfs_name: &str) -> Option<Vec<u8>> {
    let (_, ports) = sysfs_name.split_once('-')?;
    ports
        .split('.')
        .map(|port| port.parse::<u8>().ok())
        .collect()
}

fn read_trimmed(path: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}
