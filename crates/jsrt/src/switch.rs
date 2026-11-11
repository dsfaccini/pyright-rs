//! The switch of a cache or shortcut that pyright lacks (`AGENTS.md`, "gates";
//! `docs/porting.md`, "switches").

/// `on` uses the cache or shortcut, `off` runs pyright's path, and `check` runs pyright's
/// path and panics where the shortcut would have differed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SwitchMode {
    Off,
    On,
    Check,
}

impl SwitchMode {
    /// Reads the environment variable `var`: unset, empty or `on` is `On`; `off` and `check`
    /// are themselves; any other value panics.
    pub fn from_env(var: &str) -> SwitchMode {
        match std::env::var(var).as_deref() {
            Err(_) | Ok("") | Ok("on") => SwitchMode::On,
            Ok("off") => SwitchMode::Off,
            Ok("check") => SwitchMode::Check,
            Ok(other) => panic!("{var} must be on, off or check, not {other:?}"),
        }
    }
}

/// The mode of switch `$var`, read from the environment once per process.
#[macro_export]
macro_rules! switch_mode {
    ($var:literal) => {{
        static MODE: ::std::sync::OnceLock<$crate::switch::SwitchMode> = ::std::sync::OnceLock::new();
        *MODE.get_or_init(|| $crate::switch::SwitchMode::from_env($var))
    }};
}
