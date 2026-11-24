//! A session's command line, as `stablehand run` starts it: which engine a
//! command is, the model it names, and the line with the picked model's
//! flag added. No I/O: the engine table ([`Engine::start`],
//! [`Engine::model_flags`]) holds each engine's facts.
use crate::account::Engine;

/// The engine `run` starts on: the command's (its program's name), else
/// `--engine`'s, else `--account`'s; none: the whole order is walked.
/// Refused, with its fix: arguments given with no command, and a command,
/// `--engine` or `--account` that name different engines, or a command no
/// engine's with no `--engine`.
pub fn engine(
    command: &[String],
    flag: Option<Engine>,
    account: Option<(&str, Engine)>,
) -> Result<Option<Engine>, String> {
    let of_command = match command.first() {
        Some(first) if first.starts_with('-') => {
            return Err(format!(
                "`{}` are arguments with no command: they mean something else to each engine; name the command first, `stablehand run claude {}`",
                command.join(" "),
                command.join(" ")
            ));
        }
        Some(program) => match (Engine::of_program(program), flag) {
            (Some(e), _) => Some(e),
            (None, Some(_)) => None,
            (None, None) => {
                return Err(format!(
                    "`{program}` is no engine's program ({}): name its engine, `stablehand run --engine <engine> -- {program} …`",
                    programs()
                ));
            }
        },
        None => None,
    };
    let named = [
        of_command.map(|e| (format!("the command `{}`", command[0]), e)),
        flag.map(|e| ("--engine".to_string(), e)),
        account.map(|(name, e)| (format!("--account {name}"), e)),
    ];
    let named: Vec<_> = named.into_iter().flatten().collect();
    if let Some((what, e)) = named.iter().find(|(_, e)| *e != named[0].1) {
        return Err(format!(
            "{} is {}'s and {what} is {}'s: name one engine",
            named[0].0,
            named[0].1.name(),
            e.name()
        ));
    }
    Ok(named.first().map(|(_, e)| *e))
}

/// Each engine's program, as a refusal and `stablehand run --help` list them.
pub fn programs() -> String {
    let all = <Engine as clap::ValueEnum>::value_variants().iter();
    let each: Vec<_> = all
        .map(|e| format!("`{}` {}", e.start()[0], e.name()))
        .collect();
    each.join(", ")
}

/// The model `args` name by one of `engine`'s model flags (`--model opus`,
/// `--model=opus`, `-m opus`), where they do.
pub fn named_model(engine: Engine, args: &[String]) -> Option<String> {
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        for flag in engine.model_flags() {
            if arg == flag {
                return it.next().cloned();
            }
            if let Some(m) = arg.strip_prefix(&format!("{flag}=")) {
                return Some(m.to_string());
            }
        }
    }
    None
}

/// The line `run` starts: `command`, or the engine's own start line where
/// none is given, with the engine's model flag and `model` added where the
/// line names no model and the engine takes one.
pub fn line(engine: Engine, command: &[String], model: Option<&str>) -> Vec<String> {
    let mut line: Vec<String> = if command.is_empty() {
        engine.start().iter().map(|s| s.to_string()).collect()
    } else {
        command.to_vec()
    };
    let flag = engine.model_flags().first();
    if let (Some(model), Some(flag)) = (model, flag)
        && named_model(engine, &line[1..]).is_none()
    {
        line.insert(1, model.to_string());
        line.insert(1, flag.to_string());
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;
    use Engine::*;

    fn s(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn the_engine_is_the_command_s_else_the_flag_s_else_the_account_s() {
        assert_eq!(
            engine(&s(&["claude", "-p", "hi"]), None, None),
            Ok(Some(ClaudeCode))
        );
        assert_eq!(engine(&s(&["/opt/bin/codex"]), None, None), Ok(Some(Codex)));
        assert_eq!(engine(&s(&["wrap.sh"]), Some(Grok), None), Ok(Some(Grok)));
        assert_eq!(engine(&[], None, Some(("k", Kimi))), Ok(Some(Kimi)));
        assert_eq!(engine(&[], Some(Agy), Some(("a", Agy))), Ok(Some(Agy)));
        assert_eq!(engine(&[], None, None), Ok(None));
        for (cmd, flag, account, part) in [
            (s(&["-p", "hi"]), None, None, "no command"),
            (s(&["ls"]), None, None, "no engine's program"),
            (
                s(&["claude"]),
                Some(Codex),
                None,
                "claude-code's and --engine is codex's",
            ),
            (
                s(&[]),
                Some(Codex),
                Some(("k", Kimi)),
                "--account k is kimi's",
            ),
        ] {
            let err = engine(&cmd, flag, account).unwrap_err();
            assert!(err.contains(part), "{cmd:?}: {err}");
        }
    }

    #[test]
    fn the_model_flag_is_added_where_the_line_names_none() {
        assert_eq!(
            line(ClaudeCode, &[], Some("opus")),
            s(&["claude", "--model", "opus"])
        );
        assert_eq!(line(Dsh, &[], Some("x")), s(&["dsh", "tui"]));
        assert_eq!(line(Codex, &[], None), s(&["codex"]));
        assert_eq!(
            line(ClaudeCode, &s(&["claude", "-p", "hi"]), Some("opus")),
            s(&["claude", "--model", "opus", "-p", "hi"])
        );
        for named in [
            &["kimi", "-m", "k2"][..],
            &["kimi", "--model=k2"],
            &["kimi", "--model", "k2"],
        ] {
            assert_eq!(line(Kimi, &s(named), Some("other")), s(named));
            assert_eq!(named_model(Kimi, &s(&named[1..])).as_deref(), Some("k2"));
        }
        // `-m` is not claude's: its line names no model.
        assert_eq!(named_model(ClaudeCode, &s(&["-m", "x"])), None);
    }
}
