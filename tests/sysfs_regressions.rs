#![cfg(unix)]

use alienrgb::model::{DeviceKind, EffectiveAccess, HidrawNodeState, PermissionEstimate};
use alienrgb::profile::DescriptorEvidence;
use alienrgb::sysfs::discover_at;
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static FIXTURE_ID: AtomicUsize = AtomicUsize::new(0);

#[test]
fn child_symlink_cannot_escape_selected_usb_interface() {
    let fixture = Fixture::new("symlink-escape");
    let device = fixture.keyboard("1-2");
    let interface = fixture.interface(&device, "1-2:1.0", "00");
    let outside = fixture.root.join("outside");
    fs::create_dir_all(outside.join("hidraw/hidraw99")).unwrap();
    fs::write(outside.join("report_descriptor"), compatible_descriptor()).unwrap();
    symlink(
        fs::canonicalize(&outside).unwrap(),
        interface.join("driver"),
    )
    .unwrap();

    let (_, devices) = discover_at(&fixture.sys, &fixture.dev).unwrap();
    let keyboard = &devices[0];
    assert!(keyboard.hidraw.is_empty());
    assert_eq!(
        keyboard.descriptor.as_ref().unwrap().evidence,
        DescriptorEvidence::NotFound
    );
}

#[test]
fn descriptor_selection_is_sorted_bound_and_prefers_compatible_evidence() {
    let fixture = Fixture::new("descriptor-selection");
    let device = fixture.keyboard("1-3");
    let interface = fixture.interface(&device, "1-3:1.0", "00");
    fs::create_dir_all(interface.join("hid-a")).unwrap();
    fs::create_dir_all(interface.join("hid-z")).unwrap();
    fs::write(interface.join("hid-a/report_descriptor"), [0_u8]).unwrap();
    fs::write(
        interface.join("hid-z/report_descriptor"),
        compatible_descriptor(),
    )
    .unwrap();

    let (_, devices) = discover_at(&fixture.sys, &fixture.dev).unwrap();
    let descriptor = devices[0].descriptor.as_ref().unwrap();
    assert_eq!(descriptor.evidence, DescriptorEvidence::CompatibleSignature);
    assert_eq!(descriptor.interface_number.as_deref(), Some("00"));
    assert!(descriptor
        .sysfs_path
        .as_deref()
        .unwrap()
        .ends_with("hid-z/report_descriptor"));
}

#[test]
fn duplicate_known_devices_have_a_total_stable_order() {
    let fixture = Fixture::new("device-order");
    fixture.keyboard("2-1");
    fixture.keyboard("1-9");
    fixture.chassis("3-1");

    let (_, devices) = discover_at(&fixture.sys, &fixture.dev).unwrap();
    let identities = devices
        .iter()
        .map(|device| (device.kind, device.sysfs_name.as_deref().unwrap()))
        .collect::<Vec<_>>();
    assert_eq!(
        identities,
        vec![
            (DeviceKind::Keyboard, "1-9"),
            (DeviceKind::Keyboard, "2-1"),
            (DeviceKind::Chassis, "3-1"),
        ]
    );
}

#[test]
fn missing_devnode_is_distinct_from_estimated_denial() {
    let fixture = Fixture::new("permissions");
    let device = fixture.keyboard("1-4");
    let interface = fixture.interface(&device, "1-4:1.0", "00");
    fs::create_dir_all(interface.join("hid/hidraw/hidraw7")).unwrap();
    fs::create_dir_all(interface.join("hid/hidraw/hidraw8")).unwrap();
    fs::create_dir_all(&fixture.dev).unwrap();
    let denied = fixture.dev.join("hidraw8");
    fs::write(&denied, []).unwrap();
    fs::set_permissions(&denied, fs::Permissions::from_mode(0o000)).unwrap();

    let (_, devices) = discover_at(&fixture.sys, &fixture.dev).unwrap();
    let hidraw = &devices[0].hidraw;
    assert_eq!(hidraw[0].node_state, HidrawNodeState::Missing);
    assert_eq!(hidraw[0].read_access, PermissionEstimate::Unknown);
    assert_eq!(hidraw[0].write_access, PermissionEstimate::Unknown);
    assert_eq!(hidraw[0].effective_read_access, EffectiveAccess::Missing);
    assert_eq!(hidraw[0].effective_write_access, EffectiveAccess::Missing);
    assert_eq!(hidraw[1].node_state, HidrawNodeState::Present);
    assert_eq!(hidraw[1].read_access, PermissionEstimate::DeniedByModeBits);
    assert_eq!(hidraw[1].write_access, PermissionEstimate::DeniedByModeBits);
    assert_eq!(hidraw[1].effective_read_access, EffectiveAccess::Denied);
    assert_eq!(hidraw[1].effective_write_access, EffectiveAccess::Denied);

    let json = serde_json::to_value(&devices[0]).unwrap();
    assert_eq!(json["hidraw"][0]["node_state"], "missing");
    assert_eq!(json["hidraw"][0]["read_access"], "unknown");
    assert_eq!(json["hidraw"][1]["write_access"], "denied_by_mode_bits");
    assert_eq!(json["hidraw"][1]["access_basis"], "unix_mode_bits_estimate");
    assert_eq!(json["hidraw"][0]["effective_read_access"], "missing");
    assert_eq!(json["hidraw"][1]["effective_write_access"], "denied");
    assert_eq!(
        json["hidraw"][1]["effective_access_basis"],
        "faccessat_at_eaccess"
    );
}

struct Fixture {
    root: PathBuf,
    sys: PathBuf,
    dev: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let id = FIXTURE_ID.fetch_add(1, Ordering::Relaxed);
        let root = PathBuf::from(format!(
            "tests/.target/fixtures/{name}-{}-{id}",
            std::process::id()
        ));
        let sys = root.join("sys");
        let dev = root.join("dev");
        let dmi = sys.join("class/dmi/id");
        fs::create_dir_all(&dmi).unwrap();
        fs::write(dmi.join("sys_vendor"), "Alienware\n").unwrap();
        fs::write(dmi.join("product_name"), "Alienware m16 R2\n").unwrap();
        fs::write(dmi.join("bios_version"), "1.21.0\n").unwrap();
        fs::create_dir_all(sys.join("bus/usb/devices")).unwrap();
        Self { root, sys, dev }
    }

    fn keyboard(&self, name: &str) -> PathBuf {
        self.usb_device(name, "0d62", "d2b1")
    }

    fn chassis(&self, name: &str) -> PathBuf {
        self.usb_device(name, "187c", "0551")
    }

    fn usb_device(&self, name: &str, vid: &str, pid: &str) -> PathBuf {
        let path = self.sys.join("bus/usb/devices").join(name);
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("idVendor"), vid).unwrap();
        fs::write(path.join("idProduct"), pid).unwrap();
        path
    }

    fn interface(&self, device: &Path, name: &str, number: &str) -> PathBuf {
        let physical = device.join(name);
        fs::create_dir_all(&physical).unwrap();
        fs::write(physical.join("bInterfaceNumber"), number).unwrap();
        symlink(
            fs::canonicalize(&physical).unwrap(),
            self.sys.join("bus/usb/devices").join(name),
        )
        .unwrap();
        physical
    }
}

fn compatible_descriptor() -> Vec<u8> {
    vec![
        0x06, 0x89, 0xff, 0x09, 0xcc, 0xa1, 0x01, 0x85, 0xcc, 0x75, 0x08, 0x95, 0x3f, 0xb1, 0x02,
        0xc0,
    ]
}
