//! `command-line-args` 5.2.1, the argument parser pyright's CLI uses, in its
//! default (strict, case-sensitive, ungrouped) mode.
//!
//! Source: `node_modules/command-line-args/lib/{argv-tools,argv-parser,option,output}.mjs`
//! and `index.mjs`.

use regex::Regex;

/// An option value after its `type` setter ran.
#[derive(Clone, Debug, PartialEq)]
pub enum ArgValue {
    Null,
    Bool(bool),
    String(String),
    Number(f64),
    Array(Vec<ArgValue>),
}

/// The `type` setter of an option definition.
#[derive(Clone, Copy)]
pub enum OptionType {
    Boolean,
    String,
    /// A custom setter, such as pyright's `parseThreadsArgValue`.
    Custom(fn(&str) -> ArgValue),
}

#[derive(Clone, Copy)]
pub struct OptionDefinition {
    pub name: &'static str,
    pub alias: Option<char>,
    pub option_type: OptionType,
    pub multiple: bool,
    pub default_option: bool,
}

impl OptionDefinition {
    pub const fn new(name: &'static str, option_type: OptionType) -> OptionDefinition {
        OptionDefinition { name, alias: None, option_type, multiple: false, default_option: false }
    }

    pub const fn alias(mut self, alias: char) -> OptionDefinition {
        self.alias = Some(alias);
        self
    }

    pub const fn multiple_default(mut self) -> OptionDefinition {
        self.multiple = true;
        self.default_option = true;
        self
    }

    fn is_boolean(&self) -> bool {
        matches!(self.option_type, OptionType::Boolean)
    }

    fn apply_type(&self, value: &str) -> ArgValue {
        match self.option_type {
            OptionType::Boolean => ArgValue::Bool(!value.is_empty()),
            OptionType::String => ArgValue::String(value.to_string()),
            OptionType::Custom(setter) => setter(value),
        }
    }
}

/// The errors `commandLineArgs` throws, by `err.name`.
#[derive(Clone, Debug, PartialEq)]
pub enum CommandLineArgsError {
    /// `UNKNOWN_OPTION`; `optionName` is the argument as given, e.g. `--one`.
    UnknownOption { option_name: String },
    /// `UNKNOWN_VALUE`; it has no `optionName`.
    UnknownValue { value: String },
    /// `ALREADY_SET`; `optionName` is the definition's name, e.g. `one`.
    AlreadySet { option_name: String },
}

impl CommandLineArgsError {
    /// `err.optionName`.
    pub fn option_name(&self) -> Option<&str> {
        match self {
            CommandLineArgsError::UnknownOption { option_name } | CommandLineArgsError::AlreadySet { option_name } => {
                Some(option_name)
            }
            CommandLineArgsError::UnknownValue { .. } => None,
        }
    }
}

struct Patterns {
    short: Regex,
    long: Regex,
    combined_short: Regex,
    opt_equals: Regex,
}

fn patterns() -> &'static Patterns {
    thread_local! {
        static P: &'static Patterns = Box::leak(Box::new(Patterns {
            short: Regex::new(r"^-([^\d-])$").unwrap(),
            long: Regex::new(r"^--(\S+)").unwrap(),
            combined_short: Regex::new(r"^-[^\d-]{2,}$").unwrap(),
            opt_equals: Regex::new(r"^(--\S+?)=((?s).*)").unwrap(),
        }));
    }
    P.with(|p| *p)
}

fn is_option(arg: &str) -> bool {
    let re = patterns();
    (re.short.is_match(arg) || re.long.is_match(arg)) && !re.opt_equals.is_match(arg)
}

fn is_value(arg: &str) -> bool {
    let re = patterns();
    !(is_option(arg) || re.combined_short.is_match(arg) || re.opt_equals.is_match(arg))
}

/// `Definitions.get(arg)`.
fn get_definition<'a>(definitions: &'a [OptionDefinition], arg: &str) -> Option<&'a OptionDefinition> {
    let re = patterns();
    if is_option(arg) {
        if let Some(captures) = re.short.captures(arg) {
            let short_name = &captures[1];
            return definitions.iter().find(|def| def.alias.is_some_and(|a| a.to_string() == short_name));
        }
        let option_name = &re.long.captures(arg).unwrap()[1];
        return definitions.iter().find(|def| def.name == option_name);
    }
    definitions.iter().find(|def| def.name == arg)
}

/// One output option: its value and whether it was set (`state`).
struct OptionState {
    name: &'static str,
    value: ArgValue,
    is_set: bool,
}

/// `commandLineArgs(definitions, { argv })`: the options set, in the order
/// they were first seen, as `toObject()` lists them.
pub fn command_line_args(
    definitions: &[OptionDefinition],
    argv: &[String],
) -> Result<Vec<(&'static str, ArgValue)>, CommandLineArgsError> {
    let re = patterns();

    // Expand combined short options, `-ab` to `-a -b`, keeping the original arg.
    let mut args: Vec<(String, Option<String>)> = Vec::new();
    for arg in argv {
        if re.combined_short.is_match(arg) {
            let letters = &arg[1..];
            for letter in letters.chars() {
                args.push((format!("-{letter}"), Some(format!("-{letters}"))));
            }
        } else {
            args.push((arg.clone(), None));
        }
    }

    let mut output: Vec<OptionState> = Vec::new();
    let mut def: Option<&OptionDefinition> = None;
    let mut singular_default_set = false;

    for (arg, _orig_arg) in &args {
        let mut value: Option<ArgValue> = None;
        let event_set;

        if is_option(arg) {
            def = get_definition(definitions, arg);
            match def {
                Some(d) => {
                    value = Some(if d.is_boolean() { ArgValue::Bool(true) } else { ArgValue::Null });
                    event_set = true;
                }
                None => event_set = false,
            }
            if !event_set {
                return Err(CommandLineArgsError::UnknownOption { option_name: arg.clone() });
            }
        } else if let Some(captures) = re.opt_equals.captures(arg) {
            def = get_definition(definitions, &captures[1]);
            match def {
                Some(d) if d.is_boolean() => {
                    // A boolean takes no `=value`: the parser yields the arg as an unknown value.
                    return Err(CommandLineArgsError::UnknownValue { value: arg.clone() });
                }
                Some(_) => value = Some(ArgValue::String(captures[2].to_string())),
                None => return Err(CommandLineArgsError::UnknownOption { option_name: arg.clone() }),
            }
        } else if is_value(arg) {
            if def.is_some() {
                value = Some(ArgValue::String(arg.clone()));
            } else {
                def = definitions.iter().find(|d| d.default_option);
                if def.is_some() && !singular_default_set {
                    value = Some(ArgValue::String(arg.clone()));
                } else {
                    return Err(CommandLineArgsError::UnknownValue { value: arg.clone() });
                }
            }
        }

        let d = def.unwrap();

        // `option.set(argInfo.value)`.
        let index = match output.iter().position(|o| o.name == d.name) {
            Some(index) => index,
            None => {
                output.push(OptionState {
                    name: d.name,
                    value: if d.multiple { ArgValue::Array(Vec::new()) } else { ArgValue::Null },
                    is_set: false,
                });
                output.len() - 1
            }
        };
        let option = &mut output[index];
        if d.multiple {
            if let Some(ArgValue::String(v)) = &value {
                let typed = d.apply_type(v);
                if let ArgValue::Array(items) = &mut option.value {
                    items.push(typed);
                }
                option.is_set = true;
            }
        } else if option.is_set {
            return Err(CommandLineArgsError::AlreadySet { option_name: d.name.to_string() });
        } else if d.is_boolean() {
            // A flag option always sets `true`.
            option.value = ArgValue::Bool(true);
            option.is_set = true;
        } else {
            match &value {
                None | Some(ArgValue::Null) => option.value = ArgValue::Null,
                Some(ArgValue::String(v)) => {
                    option.value = d.apply_type(v);
                    option.is_set = true;
                }
                Some(other) => {
                    option.value = other.clone();
                    option.is_set = true;
                }
            }
        }

        if d.default_option && !d.multiple {
            singular_default_set = true;
        }

        // Reset the definition once its value is consumed.
        // A boolean takes no value; a singular option takes one.
        if d.is_boolean() || (!d.multiple && value.as_ref().is_some_and(|v| *v != ArgValue::Null)) {
            def = None;
        }
    }

    Ok(output.into_iter().map(|o| (o.name, o.value)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn threads(input: &str) -> ArgValue {
        input.parse::<f64>().map(ArgValue::Number).unwrap_or(ArgValue::Null)
    }

    const DEFS: [OptionDefinition; 5] = [
        OptionDefinition::new("files", OptionType::String).multiple_default(),
        OptionDefinition::new("outputjson", OptionType::Boolean),
        OptionDefinition::new("project", OptionType::String).alias('p'),
        OptionDefinition::new("threads", OptionType::Custom(threads)),
        OptionDefinition::new("watch", OptionType::Boolean).alias('w'),
    ];

    fn parse(args: &[&str]) -> Result<Vec<(&'static str, ArgValue)>, CommandLineArgsError> {
        let argv: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        command_line_args(&DEFS, &argv)
    }

    fn s(v: &str) -> ArgValue {
        ArgValue::String(v.to_string())
    }

    #[test]
    fn options_values_and_files() {
        assert_eq!(
            parse(&["a.py", "--outputjson", "-p", "x.json", "b.py"]).unwrap(),
            vec![
                ("files", ArgValue::Array(vec![s("a.py"), s("b.py")])),
                ("outputjson", ArgValue::Bool(true)),
                ("project", s("x.json")),
            ]
        );
        assert_eq!(parse(&["--project=x=y"]).unwrap(), vec![("project", s("x=y"))]);
        assert_eq!(parse(&["-"]).unwrap(), vec![("files", ArgValue::Array(vec![s("-")]))]);
    }

    #[test]
    fn missing_values_stay_null() {
        assert_eq!(
            parse(&["--project", "--outputjson"]).unwrap(),
            vec![("project", ArgValue::Null), ("outputjson", ArgValue::Bool(true))]
        );
        assert_eq!(parse(&["--threads"]).unwrap(), vec![("threads", ArgValue::Null)]);
        // A value after a valueless singular option is its value, not a file.
        assert_eq!(parse(&["--threads", "src"]).unwrap(), vec![("threads", ArgValue::Null)]);
        assert_eq!(parse(&["--threads", "4"]).unwrap(), vec![("threads", ArgValue::Number(4.0))]);
    }

    #[test]
    fn errors() {
        assert_eq!(
            parse(&["--nope"]),
            Err(CommandLineArgsError::UnknownOption { option_name: "--nope".to_string() })
        );
        assert_eq!(
            parse(&["--outputjson", "--outputjson"]),
            Err(CommandLineArgsError::AlreadySet { option_name: "outputjson".to_string() })
        );
        assert_eq!(
            parse(&["--outputjson=true"]),
            Err(CommandLineArgsError::UnknownValue { value: "--outputjson=true".to_string() })
        );
        assert_eq!(parse(&["-wx"]), Err(CommandLineArgsError::UnknownOption { option_name: "-x".to_string() }));
        assert_eq!(parse(&["-wp", "c"]).unwrap(), vec![("watch", ArgValue::Bool(true)), ("project", s("c"))]);
    }
}
