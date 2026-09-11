use crate::model::{KnownValidationRecord, RgbValue, ValidationMode, ValidationState};
use std::collections::HashSet;
use std::error::Error;
use std::fmt;

// Translated from the Alienware m16R2 (US) block in MIT-licensed
// T-Troll/alienfx-tools commit 52713b238066d1343a492018ded546ff751cfcd4,
// alienfx-gui/Mappings/devices.csv:1239-1245 (Copyright (c) 2020 Rik Lain).
// https://github.com/T-Troll/alienfx-tools/blob/master/alienfx-gui/Mappings/devices.csv
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TargetDefinition {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub logical_id: u8,
    pub validation: ValidationState,
    pub validation_detail: &'static str,
    pub known_validation: Option<KnownValidationRecord>,
}

const MAPPING_ONLY: ValidationState = ValidationState::MappingDerivedUnvalidated;
const KEYBOARD_MAPPING_ONLY_DETAIL: &str = "Mapping-derived individual address identity; whole-keyboard uniform static red coverage was observed across all 85 known targets, but that same-color all-target execution does not disambiguate this logical ID individually.";
const POWER_MAPPING_ONLY_DETAIL: &str = "Mapping-derived target; ordinary static/address semantics remain unvalidated. The attached record covers only the dedicated equal-color #ff0000 power profile and its currently observed battery-on/discharging behavior.";
const KEYBOARD_INDIVIDUAL_VALIDATION_DETAIL: &str = "The individual key address was physically validated only for one exact static #ff0000 execution on the pinned keyboard profile.";
const TOUCHPAD_VALIDATION_DETAIL: &str = "The target remains generally mapping-derived beyond the attached record, which covers only one exact static #ff0000 execution.";
const BACK_VALIDATION_DETAIL: &str = "The target remains generally mapping-derived beyond the attached record, which covers only one exact static #ff0000 execution.";
const fn keyboard_validation_record(
    target: &'static str,
    logical_id: u8,
    evidence: &'static str,
) -> KnownValidationRecord {
    KnownValidationRecord {
        mode: ValidationMode::StaticColor,
        color: "#ff0000",
        device_profile: "Alienware m16 R2",
        bios_version: "1.21.0",
        controller: "0d62:d2b1",
        target,
        logical_id,
        evidence,
    }
}

const TOUCHPAD_RED_VALIDATION: KnownValidationRecord = KnownValidationRecord {
    mode: ValidationMode::StaticColor,
    color: "#ff0000",
    device_profile: "Alienware m16 R2",
    bios_version: "1.21.0",
    controller: "187c:0551",
    target: "touchpad",
    logical_id: 0,
    evidence: "One authorized execution transferred remove/start/set_color/finish_play exactly once at 33 bytes each; the user visually confirmed red. No retry, readback, persistence, or automatic restore.",
};

const BACK_RED_VALIDATION: KnownValidationRecord = KnownValidationRecord {
    mode: ValidationMode::StaticColor,
    color: "#ff0000",
    device_profile: "Alienware m16 R2",
    bios_version: "1.21.0",
    controller: "187c:0551",
    target: "back",
    logical_id: 2,
    evidence: "One separately authorized guarded execution transferred remove/start/set_color/finish_play exactly once at 33 bytes each; the user confirmed only rear lighting became red. No retry, sudo, readback, restore, or persistence.",
};

const POWER_PROFILE_RED_VALIDATION: KnownValidationRecord = KnownValidationRecord {
    mode: ValidationMode::PowerProfileEqualColor,
    color: "#ff0000",
    device_profile: "Alienware m16 R2",
    bios_version: "1.21.0",
    controller: "187c:0551",
    target: "power",
    logical_id: 4,
    evidence: "One dedicated equal-color power profile #ff0000 execution completed all 34 ordered 33-byte writes once with no retry or status polling; the user immediately confirmed the power button red with no other zone change while sysfs reported AC online=0, BAT0 Discharging, capacity 22%. Only battery-on/discharging visible behavior was observed; AC, sleep, charging, and battery-critical visual behavior remain unobserved. Persistence unknown; no readback or restore.",
};

const ESCAPE_RED_VALIDATION: KnownValidationRecord = keyboard_validation_record(
    "escape",
    0,
    "One individually executed static #ff0000 operation completed once after ready signature cc9317112100; the user visually confirmed only Escape red. No retry, readback, persistence, or automatic restore.",
);
const F1_RED_VALIDATION: KnownValidationRecord = keyboard_validation_record(
    "f1",
    1,
    "One individually executed static #ff0000 operation completed once after ready signature cc9317112100; the user visually confirmed only F1 red. No retry, readback, persistence, or automatic restore.",
);
const W_RED_VALIDATION: KnownValidationRecord = keyboard_validation_record(
    "w",
    43,
    "One individually executed static #ff0000 operation completed once after ready signature cc9317112100; the user visually confirmed only W red. No retry, readback, persistence, or automatic restore.",
);
const SPACE_RED_VALIDATION: KnownValidationRecord = keyboard_validation_record(
    "space",
    106,
    "One individually executed static #ff0000 operation completed once after ready signature cc9317112100; the user visually confirmed the whole Space bar red. No retry, readback, persistence, or automatic restore.",
);

const KEYBOARD_TARGETS: &[TargetDefinition] = &[
    target_with_validation_record(
        "escape",
        &["esc"],
        0,
        KEYBOARD_INDIVIDUAL_VALIDATION_DETAIL,
        ESCAPE_RED_VALIDATION,
    ),
    target_with_validation_record(
        "f1",
        &[],
        1,
        KEYBOARD_INDIVIDUAL_VALIDATION_DETAIL,
        F1_RED_VALIDATION,
    ),
    target("f2", &[], 2),
    target("f3", &[], 3),
    target("f4", &[], 4),
    target("f5", &[], 5),
    target("f6", &[], 6),
    target("f7", &[], 7),
    target("f8", &[], 8),
    target("f9", &[], 9),
    target("f10", &[], 10),
    target("f11", &[], 11),
    target("f12", &[], 12),
    target("home", &[], 13),
    target("end", &[], 14),
    target("delete", &["del"], 15),
    target("mute", &[], 16),
    target("volume-down", &["vol-down"], 17),
    target("volume-up", &["vol-up"], 18),
    target("mic-mute", &["microphone-mute"], 19),
    target("grave", &["backtick"], 20),
    target("1", &[], 21),
    target("2", &[], 22),
    target("3", &[], 23),
    target("4", &[], 24),
    target("5", &[], 25),
    target("6", &[], 26),
    target("7", &[], 27),
    target("8", &[], 28),
    target("9", &[], 29),
    target("0", &[], 30),
    target("minus", &["-"], 31),
    target("equal", &["="], 32),
    target("backspace", &[], 34),
    target("tab", &[], 40),
    target("q", &[], 42),
    target_with_validation_record(
        "w",
        &[],
        43,
        KEYBOARD_INDIVIDUAL_VALIDATION_DETAIL,
        W_RED_VALIDATION,
    ),
    target("e", &[], 44),
    target("r", &[], 45),
    target("t", &[], 46),
    target("y", &[], 47),
    target("u", &[], 48),
    target("i", &[], 49),
    target("o", &[], 50),
    target("p", &[], 51),
    target("left-bracket", &["["], 52),
    target("right-bracket", &["]"], 53),
    target("backslash", &["\\"], 55),
    target("caps-lock", &[], 60),
    target("a", &[], 62),
    target("s", &[], 63),
    target("d", &[], 64),
    target("f", &[], 65),
    target("g", &[], 66),
    target("h", &[], 67),
    target("j", &[], 68),
    target("k", &[], 69),
    target("l", &[], 70),
    target("semicolon", &[";"], 71),
    target("apostrophe", &["'"], 72),
    target("enter", &["return"], 74),
    target("left-shift", &[], 80),
    target("z", &[], 83),
    target("x", &[], 84),
    target("c", &[], 85),
    target("v", &[], 86),
    target("b", &[], 87),
    target("n", &[], 88),
    target("m", &[], 89),
    target("comma", &[","], 90),
    target("period", &["."], 91),
    target("slash", &["/"], 92),
    target("right-shift", &[], 94),
    target("left-ctrl", &["ctrl"], 100),
    target("fn", &[], 101),
    target("left-meta", &["left-windows"], 102),
    target("left-alt", &["alt"], 104),
    target_with_validation_record(
        "space",
        &[],
        106,
        KEYBOARD_INDIVIDUAL_VALIDATION_DETAIL,
        SPACE_RED_VALIDATION,
    ),
    target("win-lock", &[], 109),
    target("right-alt", &[], 111),
    target("right-ctrl", &[], 112),
    target("arrow-up", &["up"], 114),
    target("arrow-left", &["left"], 133),
    target("arrow-down", &["down"], 134),
    target("arrow-right", &["right"], 135),
];

const CHASSIS_TARGETS: &[TargetDefinition] = &[
    target_with_validation_record(
        "touchpad",
        &["haptic"],
        0,
        TOUCHPAD_VALIDATION_DETAIL,
        TOUCHPAD_RED_VALIDATION,
    ),
    target_with_validation_record(
        "back",
        &["chassis"],
        2,
        BACK_VALIDATION_DETAIL,
        BACK_RED_VALIDATION,
    ),
    target_with_validation_record(
        "power",
        &["power-button"],
        4,
        POWER_MAPPING_ONLY_DETAIL,
        POWER_PROFILE_RED_VALIDATION,
    ),
];

const fn target(
    name: &'static str,
    aliases: &'static [&'static str],
    logical_id: u8,
) -> TargetDefinition {
    TargetDefinition {
        name,
        aliases,
        logical_id,
        validation: MAPPING_ONLY,
        validation_detail: KEYBOARD_MAPPING_ONLY_DETAIL,
        known_validation: None,
    }
}

const fn target_with_validation_record(
    name: &'static str,
    aliases: &'static [&'static str],
    logical_id: u8,
    validation_detail: &'static str,
    known_validation: KnownValidationRecord,
) -> TargetDefinition {
    TargetDefinition {
        name,
        aliases,
        logical_id,
        validation: MAPPING_ONLY,
        validation_detail,
        known_validation: Some(known_validation),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TargetError {
    UnknownTarget(String),
    DuplicateTarget(&'static str),
}

impl fmt::Display for TargetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownTarget(name) => write!(formatter, "unknown target '{name}'"),
            Self::DuplicateTarget(name) => write!(formatter, "duplicate target '{name}'"),
        }
    }
}

impl Error for TargetError {}

pub fn keyboard_targets() -> &'static [TargetDefinition] {
    KEYBOARD_TARGETS
}

pub fn chassis_targets() -> &'static [TargetDefinition] {
    CHASSIS_TARGETS
}

pub fn lookup_keyboard_target(name: &str) -> Option<&'static TargetDefinition> {
    lookup(KEYBOARD_TARGETS, name)
}

pub fn lookup_chassis_target(name: &str) -> Option<&'static TargetDefinition> {
    lookup(CHASSIS_TARGETS, name)
}

pub fn keyboard_target_by_id(logical_id: u8) -> Option<&'static TargetDefinition> {
    KEYBOARD_TARGETS
        .iter()
        .find(|target| target.logical_id == logical_id)
}

pub fn chassis_target_by_id(logical_id: u8) -> Option<&'static TargetDefinition> {
    CHASSIS_TARGETS
        .iter()
        .find(|target| target.logical_id == logical_id)
}

pub(crate) fn power_profile_validation(color: RgbValue) -> ValidationState {
    if color.r() == 0xff && color.g() == 0x00 && color.b() == 0x00 {
        ValidationState::ExactPowerProfileStaticRedBatteryOnObserved
    } else {
        ValidationState::MappingDerivedUnvalidated
    }
}

pub(crate) fn static_color_validation(
    target: &TargetDefinition,
    color: RgbValue,
) -> ValidationState {
    let has_exact_static_red_record = target.known_validation.is_some_and(|record| {
        record.mode == ValidationMode::StaticColor
            && record.color == "#ff0000"
            && color.r() == 0xff
            && color.g() == 0x00
            && color.b() == 0x00
    });
    if !has_exact_static_red_record {
        return ValidationState::MappingDerivedUnvalidated;
    }

    match target.name {
        "touchpad" => ValidationState::ExactTouchpadStaticRedLiveValidated,
        "back" => ValidationState::ExactBackStaticRedLiveValidated,
        _ => ValidationState::MappingDerivedUnvalidated,
    }
}

pub fn expand_keyboard_targets(
    names: &[&str],
) -> Result<Vec<&'static TargetDefinition>, TargetError> {
    expand(KEYBOARD_TARGETS, names)
}

pub fn expand_chassis_targets(
    names: &[&str],
) -> Result<Vec<&'static TargetDefinition>, TargetError> {
    expand(CHASSIS_TARGETS, names)
}

fn lookup(targets: &'static [TargetDefinition], name: &str) -> Option<&'static TargetDefinition> {
    let name = name.trim();
    targets.iter().find(|target| {
        target.name.eq_ignore_ascii_case(name)
            || target
                .aliases
                .iter()
                .any(|alias| alias.eq_ignore_ascii_case(name))
    })
}

fn expand(
    targets: &'static [TargetDefinition],
    names: &[&str],
) -> Result<Vec<&'static TargetDefinition>, TargetError> {
    let mut expanded = Vec::new();
    let mut seen = HashSet::new();
    for name in names {
        if name.trim().eq_ignore_ascii_case("all") {
            for target in targets {
                add_target(&mut expanded, &mut seen, target)?;
            }
        } else {
            let target = lookup(targets, name)
                .ok_or_else(|| TargetError::UnknownTarget((*name).to_string()))?;
            add_target(&mut expanded, &mut seen, target)?;
        }
    }
    expanded.sort_by_key(|target| target.logical_id);
    Ok(expanded)
}

fn add_target(
    expanded: &mut Vec<&'static TargetDefinition>,
    seen: &mut HashSet<u8>,
    target: &'static TargetDefinition,
) -> Result<(), TargetError> {
    if !seen.insert(target.logical_id) {
        return Err(TargetError::DuplicateTarget(target.name));
    }
    expanded.push(target);
    Ok(())
}
