use serde::Serialize;

pub const KEYBOARD_VID: &str = "0d62";
pub const KEYBOARD_PID: &str = "d2b1";
pub const CHASSIS_VID: &str = "187c";
pub const CHASSIS_PID: &str = "0551";
pub const KEYBOARD_DESCRIPTOR_SHA256: &str =
    "b552e49c3a7ed64aba7c2f1889a2563b17bf80ad270430ad4d5954237c9bb24f";
pub const CONFIRMED_BIOS_VERSION: &str = "1.21.0";
pub const KEYBOARD_OBSERVED_STATUS_READY_SIGNATURE: [u8; 6] = [0xcc, 0x93, 0x17, 0x11, 0x21, 0x00];

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DescriptorEvidence {
    HashMatch,
    CompatibleSignature,
    Mismatch,
    Unreadable,
    NotFound,
}

pub fn match_system(vendor: &str, product: &str) -> bool {
    vendor.trim().eq_ignore_ascii_case("Alienware")
        && product.trim().eq_ignore_ascii_case("Alienware m16 R2")
}

pub fn descriptor_evidence(descriptor: &[u8], sha256: Option<&str>) -> DescriptorEvidence {
    if sha256.is_some_and(|hash| hash.eq_ignore_ascii_case(KEYBOARD_DESCRIPTOR_SHA256)) {
        DescriptorEvidence::HashMatch
    } else if has_keyboard_feature_signature(descriptor) {
        DescriptorEvidence::CompatibleSignature
    } else {
        DescriptorEvidence::Mismatch
    }
}

pub fn has_keyboard_feature_signature(descriptor: &[u8]) -> bool {
    const MAX_STACK_DEPTH: usize = 16;

    #[derive(Clone, Copy, Default)]
    struct Globals {
        usage_page: u32,
        report_id: u32,
        report_size: u32,
        report_count: u32,
    }

    let mut offset = 0;
    let mut globals = Globals::default();
    let mut global_stack = Vec::new();
    let mut collections = Vec::new();
    let mut local_vendor_usage = false;
    let mut matching_feature = false;

    while offset < descriptor.len() {
        let prefix = descriptor[offset];
        offset += 1;
        if prefix == 0xfe {
            if offset + 2 > descriptor.len() {
                return false;
            }
            let size = descriptor[offset] as usize;
            offset += 2;
            if offset + size > descriptor.len() {
                return false;
            }
            offset += size;
            continue;
        }

        let size = match prefix & 0x03 {
            0 => 0,
            1 => 1,
            2 => 2,
            3 => 4,
            _ => unreachable!(),
        };
        if offset + size > descriptor.len() {
            return false;
        }
        let value = item_value(&descriptor[offset..offset + size]);
        offset += size;
        let item_type = (prefix >> 2) & 0x03;
        let tag = (prefix >> 4) & 0x0f;

        match (item_type, tag) {
            (1, 0) => globals.usage_page = value,
            (1, 7) => globals.report_size = value,
            (1, 8) => globals.report_id = value,
            (1, 9) => globals.report_count = value,
            (1, 10) => {
                if global_stack.len() == MAX_STACK_DEPTH {
                    return false;
                }
                global_stack.push(globals);
            }
            (1, 11) => {
                let Some(restored) = global_stack.pop() else {
                    return false;
                };
                globals = restored;
            }
            (2, 0) if globals.usage_page == 0xff89 && value == 0xcc => {
                local_vendor_usage = true;
            }
            (0, 10) => {
                if collections.len() == MAX_STACK_DEPTH {
                    return false;
                }
                let vendor_collection =
                    local_vendor_usage || collections.last().copied().unwrap_or(false);
                collections.push(vendor_collection);
                local_vendor_usage = false;
            }
            (0, 11) => {
                if collections.last().copied().unwrap_or(false)
                    && globals.report_id == 0xcc
                    && globals.report_size == 8
                    && globals.report_count == 63
                {
                    matching_feature = true;
                }
                local_vendor_usage = false;
            }
            (0, 12) => {
                if collections.pop().is_none() {
                    return false;
                }
                local_vendor_usage = false;
            }
            (0, _) => local_vendor_usage = false,
            _ => {}
        }
    }
    matching_feature && collections.is_empty() && global_stack.is_empty()
}

fn item_value(bytes: &[u8]) -> u32 {
    bytes
        .iter()
        .enumerate()
        .fold(0_u32, |value, (index, byte)| {
            value | (u32::from(*byte) << (index * 8))
        })
}
