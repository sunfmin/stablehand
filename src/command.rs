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

/// One `command set` argument, `<engine>=<command>`: the engine and its
/// command's words, split as `sh` would (none: drop the one kept). Refused,
/// with its fix: no `=`, an engine stablehand does not know, a command `sh`
/// cannot split, whose program is not the engine's (its name or a path
/// ending in it), or that names a model (the model is the pick's).
pub fn kept(arg: &str) -> Result<(Engine, Vec<String>), String> {
    let (name, command) = arg
        .split_once('=')
        .ok_or_else(|| format!("`{arg}` is no command: give it as <engine>=<command>, `claude-code=claude --dangerously-skip-permissions`, or <engine>= to drop one"))?;
    let engine = <Engine as clap::ValueEnum>::from_str(name.trim(), false)
        .map_err(|_| format!("no engine `{name}`: give one of {}", programs()))?;
    let words = shell_words::split(command)
        .map_err(|e| format!("{name}: sh cannot split `{command}` ({e}): close its quotes"))?;
    let Some(program) = words.first() else {
        return Ok((engine, words));
    };
    if Engine::of_program(program) != Some(engine) {
        return Err(format!(
            "{name}: `{program}` is not {name}'s program `{}` (nor a path ending in it): its command starts with its program ({})",
            engine.start()[0],
            programs()
        ));
    }
    if let Some(model) = named_model(engine, &words[1..]) {
        return Err(format!(
            "{name}: `{command}` names a model ({model}): the model is the pick's (`stablehand order set`, `run --model`); drop it from the command"
        ));
    }
    Ok((engine, words))
}

/// The kept commands after `command set`: each of `given` in place of its
/// engine's in `kept`, an empty one dropping it; by engine name.
pub fn merged(
    mut kept: Vec<(Engine, Vec<String>)>,
    given: Vec<(Engine, Vec<String>)>,
) -> Vec<(Engine, Vec<String>)> {
    for (engine, words) in given {
        kept.retain(|(e, _)| *e != engine);
        kept.extend((!words.is_empty()).then_some((engine, words)));
    }
    kept.sort_by_key(|(e, _)| e.name());
    kept
}

/// A kept command as `command show` answers it: its words joined as `sh`
/// splits them back.
pub fn shown(words: &[String]) -> String {
    shell_words::join(words)
}

/// The line `run` starts, and whether `kept` (the engine's kept command,
/// empty for none) is in it: with no command, `kept`, else the engine's own
/// start line; a command of the engine's program gets `kept`'s arguments
/// right after its program, before its own, and `kept`'s program where it
/// names a bare one; a command of another program (`--engine`'s wrapper)
/// none. Then the engine's model flag and `model`, after the program, where
/// the engine takes one (the caller adds none to a line naming its own).
pub fn line(
    engine: Engine,
    command: &[String],
    kept: &[String],
    model: Option<&str>,
) -> (Vec<String>, bool) {
    let (mut line, used) = match (command.split_first(), kept.split_first()) {
        (None, Some(_)) => (kept.to_vec(), true),
        (None, None) => (
            engine.start().iter().map(|s| s.to_string()).collect(),
            false,
        ),
        (Some((program, args)), Some((kept_program, kept_args)))
            if Engine::of_program(program) == Some(engine) =>
        {
            let program = if program.contains('/') {
                program
            } else {
                kept_program
            };
            let words = [std::slice::from_ref(program), kept_args, args].concat();
            (words, true)
        }
        (Some(_), _) => (command.to_vec(), false),
    };
    let flag = engine.model_flags().first();
    if let (Some(model), Some(flag)) = (model, flag) {
        line.insert(1, model.to_string());
        line.insert(1, flag.to_string());
    }
    (line, used)
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
        let line = |e, c: &[String], m| line(e, c, &[], m).0;
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
            assert_eq!(named_model(Kimi, &s(&named[1..])).as_deref(), Some("k2"));
        }
        // `-m` is not claude's: its line names no model.
        assert_eq!(named_model(ClaudeCode, &s(&["-m", "x"])), None);
    }

    #[test]
    fn a_kept_command_s_arguments_go_before_the_person_s_its_path_over_a_bare_name() {
        let kept = s(&["/opt/claude", "--yes"]);
        assert_eq!(
            line(ClaudeCode, &[], &kept, Some("opus")),
            (s(&["/opt/claude", "--model", "opus", "--yes"]), true)
        );
        assert_eq!(
            line(ClaudeCode, &s(&["claude", "-p", "hi"]), &kept, None),
            (s(&["/opt/claude", "--yes", "-p", "hi"]), true)
        );
        assert_eq!(
            line(ClaudeCode, &s(&["/usr/bin/claude"]), &kept, None),
            (s(&["/usr/bin/claude", "--yes"]), true)
        );
        // A wrapper `--engine` names is not the engine's program: no kept arguments.
        assert_eq!(
            line(ClaudeCode, &s(&["wrap.sh", "x"]), &kept, None),
            (s(&["wrap.sh", "x"]), false)
        );
    }

    #[test]
    fn a_kept_command_is_its_engine_s_program_and_names_no_model() {
        assert_eq!(
            kept("grok=/opt/grok --always-approve 'a b'"),
            Ok((Grok, s(&["/opt/grok", "--always-approve", "a b"])))
        );
        assert_eq!(kept("codex="), Ok((Codex, vec![])));
        assert_eq!(shown(&s(&["/opt/grok", "a b"])), "/opt/grok 'a b'");
        for (arg, part) in [
            ("x", "<engine>=<command>"),
            ("nope=claude", "no engine `nope`"),
            ("codex=claude", "not codex's program `codex`"),
            ("claude-code=claude \"x", "sh cannot split"),
            ("kimi=kimi -m k2", "names a model (k2)"),
        ] {
            let err = kept(arg).unwrap_err();
            assert!(err.contains(part), "{arg}: {err}");
        }
    }
}
