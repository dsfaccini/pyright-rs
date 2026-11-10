//! Port of `packages/pyright-internal/src/common/debug.ts`.
//!
//! A failed assertion is a pyright crash. The port does not reproduce the
//! crash output, so `fail` panics: per `docs/porting.md`, a panic is a port bug.

// common/debug.ts:13
#[track_caller]
pub fn assert(expression: bool, message: Option<&str>) {
    if !expression {
        match message {
            Some(message) => fail(Some(&format!("False expression: {message}"))),
            None => fail(Some("False expression.")),
        }
    }
}

// common/debug.ts:29
#[track_caller]
pub fn fail(message: Option<&str>) -> ! {
    match message {
        Some(message) => panic!("Debug Failure. {message}"),
        None => panic!("Debug Failure."),
    }
}

// common/debug.ts:38
#[track_caller]
pub fn assert_defined<T>(value: Option<T>, message: Option<&str>) -> T {
    match value {
        Some(value) => value,
        None => fail(message),
    }
}

// common/debug.ts:58
#[track_caller]
pub fn assert_never(member: impl std::fmt::Debug, message: Option<&str>) -> ! {
    let message = message.unwrap_or("Illegal value:");
    fail(Some(&format!("{message} {member:?}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[should_panic(expected = "Debug Failure. False expression: boom")]
    fn assert_panics_with_pyright_message() {
        assert(false, Some("boom"));
    }

    #[test]
    fn assert_passes() {
        assert(true, None);
        assert_eq!(assert_defined(Some(3), None), 3);
    }
}
