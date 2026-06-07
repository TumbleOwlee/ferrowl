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
    Set { register: String, value: String },
    Write(Option<String>),
    WriteDevice(Option<String>),
    Log(Option<String>),
    Add,
    Compact,
    Reload,
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
        "set" => Cmd::Set {
            register: args.first().copied().unwrap_or_default().to_string(),
            value: args.get(1..).map(|rest| rest.join(" ")).unwrap_or_default(),
        },
        "s" | "save" | "w" | "write" => Cmd::Write(first()),
        "wd" | "write-device" => Cmd::WriteDevice(first()),
        "log" => Cmd::Log(first()),
        "a" | "add" => Cmd::Add,
        "compact" => Cmd::Compact,
        "reload" => Cmd::Reload,
        other => Cmd::Unknown(other.to_string()),
    }
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
    fn ut_unknown() {
        assert_eq!(parse("bogus"), Cmd::Unknown("bogus".to_string()));
    }
}
