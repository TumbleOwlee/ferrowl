//! Parser for the vim-style `:` command line. Pure (no app state) so it is unit-testable;
//! execution lives in `App::run_command`.

/// A parsed command-line command.
#[derive(Debug, PartialEq, Eq)]
pub enum Cmd {
    Quit,
    QuitAll,
    Edit,
    New,
    Load(Option<String>),
    Start,
    Stop,
    Restart,
    Lua(LuaCommand),
    Set {
        register: String,
        value: String,
    },
    Write(Option<String>),
    WriteDevice(Option<String>),
    Log(Option<String>),
    Add,
    Compact,
    Reload,
    Order {
        column: Option<String>,
        descending: bool,
    },
    Unknown(String),
    Empty,
}

/// Sub-command of `:lua` — start or stop the module's Lua simulation thread.
#[derive(Debug, PartialEq, Eq)]
pub enum LuaCommand {
    Start,
    Stop,
}

/// Parse a command line (without the leading `:`).
pub fn parse(input: &str) -> Cmd {
    let mut parts = input.split_whitespace();
    let Some(name) = parts.next() else {
        return Cmd::Empty;
    };
    let args: Vec<&str> = parts.collect();
    let first = || args.first().map(|s| s.to_string());

    match name {
        "q" | "q!" | "quit" => Cmd::Quit,
        "qa" | "qa!" | "qall" => Cmd::QuitAll,
        "e" | "edit" => Cmd::Edit,
        "n" | "new" => Cmd::New,
        "l" | "load" => Cmd::Load(first()),
        "start" => Cmd::Start,
        "stop" => Cmd::Stop,
        "restart" => Cmd::Restart,
        "lua" => match args.first().copied() {
            Some("start") => Cmd::Lua(LuaCommand::Start),
            Some("stop") => Cmd::Lua(LuaCommand::Stop),
            _ => Cmd::Unknown(format!("lua {}", args.join(" ")).trim().to_string()),
        },
        "set" => {
            // Everything after the `set` token; the register name may be double-quoted to
            // allow whitespace (e.g. `set "Slave Mode" idle`).
            let rest = input
                .trim_start()
                .split_once(char::is_whitespace)
                .map(|(_, r)| r)
                .unwrap_or("")
                .trim_start();
            parse_set(rest)
        }
        "s" | "save" | "w" | "write" => Cmd::Write(first()),
        "wd" | "write-device" => Cmd::WriteDevice(first()),
        "log" => Cmd::Log(first()),
        "a" | "add" => Cmd::Add,
        "compact" => Cmd::Compact,
        "reload" => Cmd::Reload,
        "order" => {
            let mut cols = args.as_slice();
            let mut descending = false;
            if let Some((last, rest)) = cols.split_last() {
                match last.to_ascii_lowercase().as_str() {
                    "asc" => cols = rest,
                    "desc" => {
                        descending = true;
                        cols = rest;
                    }
                    _ => {}
                }
            }
            let column = if cols.is_empty() {
                None
            } else {
                Some(cols.join(" "))
            };
            Cmd::Order { column, descending }
        }
        other => Cmd::Unknown(other.to_string()),
    }
}

/// Split the `:set` argument string into `(register, value)`. A leading double-quote lets
/// the register name contain whitespace; otherwise the register is the first token.
fn parse_set(rest: &str) -> Cmd {
    let (register, value) = if let Some(after) = rest.strip_prefix('"') {
        match after.split_once('"') {
            Some((reg, val)) => (reg.to_string(), val.trim_start().to_string()),
            None => (after.to_string(), String::new()), // unterminated quote
        }
    } else {
        match rest.split_once(char::is_whitespace) {
            Some((reg, val)) => (reg.to_string(), val.trim_start().to_string()),
            None => (rest.to_string(), String::new()),
        }
    };
    Cmd::Set { register, value }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ut_empty() {
        assert_eq!(parse(""), Cmd::Empty);
        assert_eq!(parse("   "), Cmd::Empty);
    }

    #[test]
    fn ut_simple() {
        assert_eq!(parse("q"), Cmd::Quit);
        assert_eq!(parse("quit"), Cmd::Quit);
        assert_eq!(parse("q!"), Cmd::Quit);
        assert_eq!(parse("e"), Cmd::Edit);
        assert_eq!(parse("new"), Cmd::New);
        assert_eq!(parse("start"), Cmd::Start);
        assert_eq!(parse("stop"), Cmd::Stop);
        assert_eq!(parse("restart"), Cmd::Restart);
    }

    #[test]
    fn ut_set() {
        assert_eq!(
            parse("set power 100"),
            Cmd::Set {
                register: "power".to_string(),
                value: "100".to_string()
            }
        );
        // value may contain spaces (joined back).
        assert_eq!(
            parse("set label hello world"),
            Cmd::Set {
                register: "label".to_string(),
                value: "hello world".to_string()
            }
        );
        // missing value -> empty value (caller reports usage).
        assert_eq!(
            parse("set power"),
            Cmd::Set {
                register: "power".to_string(),
                value: String::new()
            }
        );
    }

    #[test]
    fn ut_set_quoted() {
        // quoted register name with whitespace.
        assert_eq!(
            parse("set \"Slave Mode\" idle"),
            Cmd::Set {
                register: "Slave Mode".to_string(),
                value: "idle".to_string()
            }
        );
        // quoted name + value with spaces.
        assert_eq!(
            parse("set \"My Reg\" hello world"),
            Cmd::Set {
                register: "My Reg".to_string(),
                value: "hello world".to_string()
            }
        );
        // quoted name, missing value.
        assert_eq!(
            parse("set \"My Reg\""),
            Cmd::Set {
                register: "My Reg".to_string(),
                value: String::new()
            }
        );
        // unterminated quote -> whole remainder is the register name.
        assert_eq!(
            parse("set \"My Reg"),
            Cmd::Set {
                register: "My Reg".to_string(),
                value: String::new()
            }
        );
    }

    #[test]
    fn ut_optional_paths() {
        assert_eq!(parse("w"), Cmd::Write(None));
        assert_eq!(
            parse("write out.toml"),
            Cmd::Write(Some("out.toml".to_string()))
        );
        assert_eq!(
            parse("load dev.toml"),
            Cmd::Load(Some("dev.toml".to_string()))
        );
        assert_eq!(parse("log run.log"), Cmd::Log(Some("run.log".to_string())));
        assert_eq!(parse("log"), Cmd::Log(None));
    }

    #[test]
    fn ut_lua() {
        assert_eq!(parse("lua start"), Cmd::Lua(LuaCommand::Start));
        assert_eq!(parse("lua stop"), Cmd::Lua(LuaCommand::Stop));
        // Missing/invalid subcommand falls through to Unknown.
        assert_eq!(parse("lua"), Cmd::Unknown("lua".to_string()));
        assert_eq!(parse("lua wat"), Cmd::Unknown("lua wat".to_string()));
    }

    #[test]
    fn ut_order() {
        // bare -> clear
        assert_eq!(
            parse("order"),
            Cmd::Order {
                column: None,
                descending: false
            }
        );
        // column only -> ASC default
        assert_eq!(
            parse("order name"),
            Cmd::Order {
                column: Some("name".to_string()),
                descending: false
            }
        );
        // explicit ASC stripped
        assert_eq!(
            parse("order Address ASC"),
            Cmd::Order {
                column: Some("Address".to_string()),
                descending: false
            }
        );
        // multi-word column + DESC (case-insensitive)
        assert_eq!(
            parse("order slave id desc"),
            Cmd::Order {
                column: Some("slave id".to_string()),
                descending: true
            }
        );
        // direction-only -> treated as clear
        assert_eq!(
            parse("order desc"),
            Cmd::Order {
                column: None,
                descending: true
            }
        );
    }

    #[test]
    fn ut_unknown() {
        assert_eq!(parse("bogus"), Cmd::Unknown("bogus".to_string()));
    }
}
