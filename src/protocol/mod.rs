pub mod api_v4;
pub mod api_v5;
pub mod power_v4;

use crate::model::RgbValue;
use std::error::Error;
use std::fmt;

pub type Rgb = RgbValue;

impl RgbValue {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    pub fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LogicalColor {
    pub logical_id: u8,
    pub color: Rgb,
}

impl LogicalColor {
    pub const fn new(logical_id: u8, color: Rgb) -> Self {
        Self { logical_id, color }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EncodeError {
    EmptyAssignments,
    DuplicateLogicalId(u8),
    UnknownLogicalId(u8),
    EncodedIdOutOfRange(u8),
}

impl fmt::Display for EncodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyAssignments => write!(formatter, "at least one assignment is required"),
            Self::DuplicateLogicalId(id) => write!(formatter, "duplicate logical target ID {id}"),
            Self::UnknownLogicalId(id) => write!(formatter, "unknown logical target ID {id}"),
            Self::EncodedIdOutOfRange(id) => {
                write!(
                    formatter,
                    "logical target ID {id} cannot be encoded as ID + 1"
                )
            }
        }
    }
}

impl Error for EncodeError {}

pub(crate) fn padded_hex(prefix: &[u8], length: usize) -> String {
    debug_assert!(prefix.len() <= length);
    let mut result = String::with_capacity(length * 2);
    for byte in prefix {
        result.push_str(&format!("{byte:02x}"));
    }
    result.push_str(&"00".repeat(length - prefix.len()));
    result
}
