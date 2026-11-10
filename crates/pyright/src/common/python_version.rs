//! Port of `packages/pyright-internal/src/common/pythonVersion.ts`.

use jsrt::JsStr;

// common/pythonVersion.ts:10
/// Ordered like its string values: `alpha < beta < candidate < final`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PythonReleaseLevel {
    Alpha,
    Beta,
    Candidate,
    Final,
}

impl PythonReleaseLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            PythonReleaseLevel::Alpha => "alpha",
            PythonReleaseLevel::Beta => "beta",
            PythonReleaseLevel::Candidate => "candidate",
            PythonReleaseLevel::Final => "final",
        }
    }

    pub fn from_name(value: &str) -> Option<PythonReleaseLevel> {
        match value {
            "alpha" => Some(PythonReleaseLevel::Alpha),
            "beta" => Some(PythonReleaseLevel::Beta),
            "candidate" => Some(PythonReleaseLevel::Candidate),
            "final" => Some(PythonReleaseLevel::Final),
            _ => None,
        }
    }
}

// common/pythonVersion.ts:12
#[derive(Clone, Copy, Debug, PartialEq, Hash, Eq)]
pub struct PythonVersion {
    pub major: i32,
    pub minor: i32,
    pub micro: Option<i32>,
    pub release_level: Option<PythonReleaseLevel>,
    pub serial: Option<i32>,
}

impl PythonVersion {
    // common/pythonVersion.ts:37
    pub const fn create(
        major: i32,
        minor: i32,
        micro: Option<i32>,
        release_level: Option<PythonReleaseLevel>,
        serial: Option<i32>,
    ) -> PythonVersion {
        PythonVersion {
            major,
            minor,
            micro,
            release_level,
            serial,
        }
    }

    const fn major_minor(major: i32, minor: i32) -> PythonVersion {
        PythonVersion::create(major, minor, None, None, None)
    }

    // common/pythonVersion.ts:53
    pub fn is_equal_to(version: &PythonVersion, other: &PythonVersion) -> bool {
        if version.major != other.major || version.minor != other.minor {
            return false;
        }

        match (version.micro, other.micro) {
            (Some(a), Some(b)) if a != b => return false,
            (Some(_), Some(_)) => {}
            _ => return true,
        }

        match (version.release_level, other.release_level) {
            (Some(a), Some(b)) if a != b => return false,
            (Some(_), Some(_)) => {}
            _ => return true,
        }

        match (version.serial, other.serial) {
            (Some(a), Some(b)) if a != b => return false,
            _ => true,
        }
    }

    // common/pythonVersion.ts:79
    pub fn is_greater_than(version: &PythonVersion, other: &PythonVersion) -> bool {
        if version.major > other.major {
            return true;
        } else if version.major < other.major {
            return false;
        }

        if version.minor > other.minor {
            return true;
        } else if version.minor < other.minor {
            return false;
        }

        match (version.micro, other.micro) {
            (Some(a), Some(b)) if a > b => return true,
            (Some(a), Some(b)) if a == b => {}
            _ => return false,
        }

        match (version.release_level, other.release_level) {
            (Some(a), Some(b)) if a > b => return true,
            (Some(a), Some(b)) if a == b => {}
            _ => return false,
        }

        match (version.serial, other.serial) {
            (Some(a), Some(b)) if a > b => true,
            _ => false,
        }
    }

    // common/pythonVersion.ts:121
    pub fn is_greater_or_equal_to(version: &PythonVersion, other: &PythonVersion) -> bool {
        PythonVersion::is_equal_to(version, other) || PythonVersion::is_greater_than(version, other)
    }

    // common/pythonVersion.ts:125
    pub fn is_less_than(version: &PythonVersion, other: &PythonVersion) -> bool {
        !PythonVersion::is_greater_or_equal_to(version, other)
    }

    // common/pythonVersion.ts:129
    pub fn is_less_or_equal_to(version: &PythonVersion, other: &PythonVersion) -> bool {
        !PythonVersion::is_greater_than(version, other)
    }

    // common/pythonVersion.ts:133
    pub fn to_major_minor_string(version: &PythonVersion) -> String {
        format!("{}.{}", version.major, version.minor)
    }

    // common/pythonVersion.ts:137
    pub fn to_string(version: &PythonVersion) -> String {
        let mut vers_string = PythonVersion::to_major_minor_string(version);

        let Some(micro) = version.micro else {
            return vers_string;
        };
        vers_string.push_str(&format!(".{micro}"));

        let Some(release_level) = version.release_level else {
            return vers_string;
        };
        vers_string.push_str(&format!(".{}", release_level.as_str()));

        let Some(serial) = version.serial else {
            return vers_string;
        };
        vers_string.push_str(&format!(".{serial}"));

        vers_string
    }

    // common/pythonVersion.ts:160
    pub fn from_string(val: &str) -> Option<PythonVersion> {
        let split: Vec<_> = val.js_split(".", None);
        if split.len() < 2 {
            return None;
        }

        let major = parse_int(&split[0])?;
        let minor = parse_int(&split[1])?;

        let micro = if split.len() >= 3 {
            parse_int(&split[2])
        } else {
            None
        };

        let release_level = if split.len() >= 4 {
            PythonReleaseLevel::from_name(&split[3])
        } else {
            None
        };

        let serial = if split.len() >= 5 {
            parse_int(&split[4])
        } else {
            None
        };

        Some(PythonVersion::create(
            major,
            minor,
            micro,
            release_level,
            serial,
        ))
    }
}

/// JS `parseInt(s, 10)`; `None` is `NaN`.
fn parse_int(s: &str) -> Option<i32> {
    let value = jsrt::number::parse_int(s, 10);
    if value.is_nan() {
        None
    } else {
        Some(value as i32)
    }
}

// common/pythonVersion.ts:203
pub const PYTHON_VERSION3_0: PythonVersion = PythonVersion::major_minor(3, 0);
pub const PYTHON_VERSION3_1: PythonVersion = PythonVersion::major_minor(3, 1);
pub const PYTHON_VERSION3_2: PythonVersion = PythonVersion::major_minor(3, 2);
pub const PYTHON_VERSION3_3: PythonVersion = PythonVersion::major_minor(3, 3);
pub const PYTHON_VERSION3_4: PythonVersion = PythonVersion::major_minor(3, 4);
pub const PYTHON_VERSION3_5: PythonVersion = PythonVersion::major_minor(3, 5);
pub const PYTHON_VERSION3_6: PythonVersion = PythonVersion::major_minor(3, 6);
pub const PYTHON_VERSION3_7: PythonVersion = PythonVersion::major_minor(3, 7);
pub const PYTHON_VERSION3_8: PythonVersion = PythonVersion::major_minor(3, 8);
pub const PYTHON_VERSION3_9: PythonVersion = PythonVersion::major_minor(3, 9);
pub const PYTHON_VERSION3_10: PythonVersion = PythonVersion::major_minor(3, 10);
pub const PYTHON_VERSION3_11: PythonVersion = PythonVersion::major_minor(3, 11);
pub const PYTHON_VERSION3_12: PythonVersion = PythonVersion::major_minor(3, 12);
pub const PYTHON_VERSION3_13: PythonVersion = PythonVersion::major_minor(3, 13);
pub const PYTHON_VERSION3_14: PythonVersion = PythonVersion::major_minor(3, 14);
pub const PYTHON_VERSION3_15: PythonVersion = PythonVersion::major_minor(3, 15);

// common/pythonVersion.ts:220
pub const LATEST_STABLE_PYTHON_VERSION: PythonVersion = PYTHON_VERSION3_14;

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> PythonVersion {
        PythonVersion::from_string(s).unwrap()
    }

    #[test]
    fn from_string_and_back() {
        assert_eq!(PythonVersion::to_string(&v("3.12")), "3.12");
        assert_eq!(
            PythonVersion::to_string(&v("3.12.1.beta.2")),
            "3.12.1.beta.2"
        );
        assert_eq!(PythonVersion::to_string(&v("3.12.x")), "3.12");
        assert_eq!(PythonVersion::from_string("3"), None);
        assert_eq!(PythonVersion::from_string("a.1"), None);
        assert_eq!(v("3.9rc1").minor, 9);
    }

    #[test]
    fn comparisons_skip_missing_parts() {
        assert!(PythonVersion::is_equal_to(&v("3.12"), &v("3.12.4")));
        assert!(!PythonVersion::is_greater_than(&v("3.12.4"), &v("3.12")));
        assert!(PythonVersion::is_greater_than(&v("3.12.4"), &v("3.12.3")));
        assert!(PythonVersion::is_greater_than(
            &v("3.12.4.final"),
            &v("3.12.4.beta")
        ));
        assert!(PythonVersion::is_less_than(&v("3.11"), &PYTHON_VERSION3_12));
        assert!(PythonVersion::is_greater_or_equal_to(
            &LATEST_STABLE_PYTHON_VERSION,
            &PYTHON_VERSION3_14
        ));
    }
}
