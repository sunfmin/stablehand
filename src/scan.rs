//! `account scan`: the engine logins already on this machine, and the
//! portable tokens mytokens keeps, found and, with `--add`, registered: one
//! account per subscription, told apart by the identity each login file
//! holds. An identity is read and compared in memory, and only its hash
//! ([`hash`]) is answered or kept; a token is never read (docs/adr/0001).
use crate::account::{self, Account, Change, Engine, Terms};
use crate::login;
use crate::refusal::{Code, Refused};
use crate::store::Store;
use base64::Engine as _;
use serde_json::{Value, json};
use sha2::Digest;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// One login found: its engine's login file in a home.
struct Found {
    engine: Engine,
    home: PathBuf,
    /// When its login file was last written: when it logged in or refreshed.
    written: u64,
    /// The hash of its identity, or why it cannot be read.
    identity: Result<String, String>,
}

/// What scan makes of a login or a token.
#[derive(Debug, PartialEq)]
enum Status {
    /// Not kept: `--add` registers it as this name.
    New(String),
    /// The account of this name is it.
    Registered(String),
    /// Another login of the subscription of the account of this name.
    Copy(String),
}

/// Where each engine's login holds its identity, as `account scan --help`
/// says it: the one list beside [`identity`], which reads them.
pub const IDENTITIES: &str = "claude-code: its account and organization in `.claude.json` (its home's, or `~/.claude.json` for `~/.claude`); codex: `tokens.account_id`, the ChatGPT account; pi: `openai-codex.accountId`, the same ChatGPT account; kimi: its access token's `user_id`; agy: `id_token_email`; grok: its login's `user_id`; dsh: none, its file holds only a grant";

/// The identity `engine`'s login in `home` holds, `<kind>:<id>` (codex's and
/// pi's one kind: a ChatGPT account), or why it cannot be read. Never said
/// or kept: [`hash`] of it is.
fn identity(engine: Engine, home: &Path) -> Result<String, String> {
    let file = engine.login().file;
    let json = |rel: &str| -> Result<Value, String> {
        let bytes = std::fs::read(home.join(rel)).map_err(|_| format!("{rel} cannot be read"))?;
        serde_json::from_slice(&bytes).map_err(|_| format!("{rel} is no JSON stablehand reads"))
    };
    let field = |v: &Value, pointer: &str, of: &str| {
        let id = v.pointer(pointer).and_then(Value::as_str);
        id.filter(|s| !s.is_empty())
            .map(String::from)
            .ok_or_else(|| format!("{of} holds no {pointer}, a format stablehand does not know"))
    };
    let (kind, id) = match engine {
        Engine::ClaudeCode => {
            // Claude Code keeps its account beside its login: in its home's
            // .claude.json, or for its default home, ~/.claude, in ~/.claude.json.
            let named = home.join(".claude.json");
            let rel = match (named.is_file(), home.parent()) {
                (false, Some(_)) if home.file_name() == Some(".claude".as_ref()) => {
                    "../.claude.json"
                }
                _ => ".claude.json",
            };
            let v = json(rel)?;
            let user = field(&v, "/oauthAccount/accountUuid", rel)?;
            let org = field(&v, "/oauthAccount/organizationUuid", rel)?;
            ("claude", format!("{user}/{org}"))
        }
        Engine::Codex => ("chatgpt", field(&json(file)?, "/tokens/account_id", file)?),
        Engine::Pi => (
            "chatgpt",
            field(&json(file)?, "/openai-codex/accountId", file)?,
        ),
        Engine::Kimi => {
            let token = field(&json(file)?, "/access_token", file)?;
            (
                "kimi",
                field(&jwt(&token)?, "/user_id", "its access token")?,
            )
        }
        Engine::Agy => ("agy", field(&json(file)?, "/id_token_email", file)?),
        Engine::Grok => {
            let v = json(file)?;
            let mut users = v.as_object().into_iter().flatten().map(|(_, e)| e);
            let user = users.next().ok_or(format!("{file} holds no login"))?;
            if users.next().is_some() {
                return Err(format!(
                    "{file} holds several logins: scan one home of each"
                ));
            }
            ("grok", field(user, "/user_id", file)?)
        }
        Engine::Dsh => return Err(format!("{file} holds no account id, only its grant")),
    };
    Ok(format!("{kind}:{id}"))
}

/// The claims of the JSON web token `token`.
fn jwt(token: &str) -> Result<Value, String> {
    let unreadable = || "its access token is no JWT stablehand reads".to_string();
    let payload = token.split('.').nth(1).ok_or_else(unreadable)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .map_err(|_| unreadable())?;
    serde_json::from_slice(&bytes).map_err(|_| unreadable())
}

/// An identity as it is said and kept: the first 16 hex digits of its SHA-256.
fn hash(identity: &str) -> String {
    let digest = sha2::Sha256::digest(format!("stablehand identity {identity}"));
    digest[..8].iter().map(|b| format!("{b:02x}")).collect()
}

/// `home` as a path compares: canonical where it exists.
fn same(a: &Path, b: &Path) -> bool {
    let canonical = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    canonical(a) == canonical(b)
}

/// The logins in each engine's default home under `user_home`, and in each
/// of `paths` for every engine whose login file it holds (of the engines
/// sharing a file name, those whose identity it reads, else all).
fn logins(user_home: &Path, paths: &[PathBuf]) -> Vec<Found> {
    let engines = <Engine as clap::ValueEnum>::value_variants();
    let found = |engine: Engine, home: PathBuf| {
        let written = login::written(&home.join(engine.login().file))?;
        let identity = identity(engine, &home).map(|i| hash(&i));
        Some(Found {
            engine,
            home,
            written,
            identity,
        })
    };
    let mut all: Vec<Found> = engines
        .iter()
        .filter_map(|&e| found(e, user_home.join(e.login().default_home)))
        .collect();
    for path in paths {
        let mut here: Vec<_> = engines
            .iter()
            .filter(|&&e| !all.iter().any(|f| f.engine == e && same(&f.home, path)))
            .filter_map(|&e| found(e, path.clone()))
            .collect();
        if here.iter().any(|f| f.identity.is_ok()) {
            here.retain(|f| f.identity.is_ok());
        }
        all.extend(here);
    }
    all
}

/// The first of `base`, `base-2`, `base-3` … not in `taken`, now taken.
fn fresh(taken: &mut Vec<String>, base: &str) -> String {
    let name = (1..)
        .map(|n| match n {
            1 => base.to_string(),
            n => format!("{base}-{n}"),
        })
        .find(|n| !taken.contains(n))
        .expect("some name is free");
    taken.push(name.clone());
    name
}

/// What each of `found` is, judged against the accounts `kept` (each with
/// the hash of its identity, kept or read in its home) bound to `machine`:
/// an account kept at its home is it; then, newest first, one whose
/// identity an account has is a copy of that, and the rest are new, the
/// newest of an identity its account and the others copies of it. A login
/// whose identity cannot be read is new, never a copy.
fn judge(
    found: &[Found],
    kept: &[(Account, Option<String>)],
    machine: &str,
    taken: &mut Vec<String>,
) -> Vec<Status> {
    let mut owner: HashMap<String, String> = HashMap::new();
    let mut status: Vec<Option<Status>> = found
        .iter()
        .map(|f| {
            let (a, _) = kept.iter().find(|(a, _)| {
                a.engine == f.engine
                    && a.machine.as_deref() == Some(machine)
                    && a.home.as_ref().is_some_and(|h| same(Path::new(h), &f.home))
            })?;
            if let Ok(id) = &f.identity {
                owner.entry(id.clone()).or_insert(a.name.clone());
            }
            Some(Status::Registered(a.name.clone()))
        })
        .collect();
    for (a, id) in kept {
        if let Some(id) = id {
            owner.entry(id.clone()).or_insert(a.name.clone());
        }
    }
    let mut newest: Vec<usize> = (0..found.len()).collect();
    newest.sort_by_key(|&i| std::cmp::Reverse(found[i].written));
    for i in newest {
        if status[i].is_some() {
            continue;
        }
        let f = &found[i];
        status[i] = Some(match &f.identity {
            Ok(id) => match owner.get(id) {
                Some(name) => Status::Copy(name.clone()),
                None => {
                    let name = fresh(taken, &f.engine.name());
                    owner.insert(id.clone(), name.clone());
                    Status::New(name)
                }
            },
            Err(_) => Status::New(fresh(taken, &f.engine.name())),
        });
    }
    status
        .into_iter()
        .map(|s| s.expect("each judged"))
        .collect()
}

const PREFIX: &str = "agent-account-";

/// One `agent-account-*` entry as a line of `mytokens list` lists it,
/// `<service>/<label>\t<kind>\t<level>  — <about>  fields: <F>, <F>`.
#[derive(Debug, PartialEq)]
struct Entry<'a> {
    /// `<service>/<label>`.
    id: &'a str,
    service: &'a str,
    label: &'a str,
    fields: Vec<&'a str>,
}

/// The `agent-account-*` entries of `mytokens list`'s lines.
fn entries(list: &str) -> impl Iterator<Item = Entry<'_>> {
    list.lines().filter_map(|line| {
        let (id, _) = line.split_once('\t')?;
        let (service, label) = id.split_once('/')?;
        service.strip_prefix(PREFIX).filter(|n| !n.is_empty())?;
        let fields = line.rsplit_once("  fields: ").map_or("", |(_, f)| f);
        Some(Entry {
            id,
            service,
            label,
            fields: fields.split(", ").filter(|f| !f.is_empty()).collect(),
        })
    })
}

impl Entry<'_> {
    /// The engine whose token variable is one of its fields, and that field.
    fn token(&self) -> Option<(Engine, &'static str)> {
        let engines = <Engine as clap::ValueEnum>::value_variants();
        engines.iter().find_map(|&e| {
            let var = e.login().token_var?;
            self.fields.contains(&var).then_some((e, var))
        })
    }

    /// The `mytokens get` that prints its token field `var`.
    fn token_command(&self, var: &str) -> String {
        let account = match self.label {
            "default" => String::new(),
            label => format!(" --account '{}'", label.replace('\'', "'\\''")),
        };
        format!("mytokens get {}{account} --field {var}", self.service)
    }
}

/// `mytokens list`, or why it was not read: mytokens not on PATH, or failing.
fn mytokens_list() -> Result<String, String> {
    let out = Command::new("mytokens")
        .arg("list")
        .stdin(Stdio::null())
        .stderr(Stdio::inherit())
        .output();
    match out {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err("not on PATH: its entries were not looked at".into())
        }
        Err(e) => Err(format!("`mytokens list` could not start: {e}")),
        Ok(o) if !o.status.success() => Err("`mytokens list` failed, as it said above".into()),
        Ok(o) => Ok(String::from_utf8_lossy(&o.stdout).into_owned()),
    }
}

/// The scan's answer, and with `add` the new logins and tokens registered:
/// bound to this machine in their home, or portable with their token
/// command, each a bound one with its identity's hash.
pub fn scan(store: &Store, paths: &[String], add: bool) -> Result<Value, Refused> {
    for p in paths {
        if !Path::new(p).is_dir() {
            return Err(Refused::new(
                Code::BadPath,
                format!("--path `{p}` is no directory: give a home an engine logged in to"),
            ));
        }
    }
    let user_home = std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .ok_or_else(|| {
            "$HOME is not set: run the scan as the person whose logins they are".to_string()
        })?;
    let machine = login::this_machine()?;
    let paths: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
    let found = logins(Path::new(&user_home), &paths);
    let kept: Vec<(Account, Option<String>)> = store
        .list()?
        .into_iter()
        .map(|a| {
            let read = match (&a.machine, &a.home) {
                (Some(m), Some(h)) if *m == machine => identity(a.engine, Path::new(h)).ok(),
                _ => None,
            };
            let id = a.identity.clone().or(read.map(|i| hash(&i)));
            (a, id)
        })
        .collect();
    let mut taken: Vec<String> = kept.iter().map(|(a, _)| a.name.clone()).collect();
    let judged = judge(&found, &kept, &machine, &mut taken);

    let mut out_logins = Vec::new();
    for (f, status) in found.iter().zip(&judged) {
        let (status, name) = match status {
            Status::New(n) if add => {
                let terms = Terms {
                    machine: Some(machine.clone()),
                    home: Some(f.home.display().to_string()),
                    ..Terms::default()
                };
                let change = Change {
                    terms,
                    ..Change::default()
                };
                let mut a = account::new(n, f.engine, change).map_err(|e| e.join("; "))?;
                a.identity = f.identity.clone().ok();
                insert(store, &a)?;
                ("added", n)
            }
            Status::New(n) => ("new", n),
            Status::Registered(n) => ("registered", n),
            Status::Copy(n) => ("copy", n),
        };
        let warning = (status == "copy").then(|| format!(
            "another login of {name}'s subscription: copies of one refresh token refresh each other out, so run its sessions on {name} only, and leave this home unused or log it out"
        ));
        out_logins.push(json!({
            "engine": f.engine.name(),
            "home": f.home.display().to_string(),
            "logged_in_at": crate::time::utc(f.written),
            "identity": f.identity.as_ref().ok(),
            "identity_unknown": f.identity.as_ref().err(),
            "status": status,
            "account": name,
            "warning": warning,
        }));
    }

    let (list, mytokens) = match mytokens_list() {
        Ok(list) => (list, None),
        Err(why) => (String::new(), Some(why)),
    };
    let mut tokens = Vec::new();
    for e in entries(&list) {
        let Some((engine, var)) = e.token() else {
            let why = match e.fields.contains(&"MACHINE") {
                true => {
                    "bound: its login is in a home of factorium's (~/.agent-accounts), which scan does not look in; `--path` that home to find it"
                }
                false => "it holds no engine's token field",
            };
            tokens.push(json!({ "entry": e.id, "status": "skipped", "why": why }));
            continue;
        };
        let cmd = e.token_command(var);
        let legacy = format!(
            "mytokens get {} --account {} --field {var}",
            e.service, e.label
        );
        let registered = kept.iter().find(|(a, _)| {
            a.machine.is_none()
                && a.token_command
                    .as_ref()
                    .is_some_and(|c| *c == cmd || *c == legacy)
        });
        let (status, name) = match registered {
            Some((a, _)) => ("registered", a.name.clone()),
            None => {
                let name = fresh(&mut taken, &e.service[PREFIX.len()..]);
                if add {
                    let terms = Terms {
                        token_command: Some(cmd.clone()),
                        ..Terms::default()
                    };
                    let change = Change {
                        terms,
                        ..Change::default()
                    };
                    let a = account::new(&name, engine, change).map_err(|f| f.join("; "))?;
                    insert(store, &a)?;
                    ("added", name)
                } else {
                    ("new", name)
                }
            }
        };
        tokens.push(json!({
            "entry": e.id,
            "engine": engine.name(),
            "token_command": cmd,
            "status": status,
            "account": name,
            // A portable token tells no identity (a setup token is refused
            // user:profile): whether it is a home login's subscription is not known.
            "same_as_a_login": "unknown",
        }));
    }
    Ok(json!({
        "add": add,
        "machine": machine,
        "logins": out_logins,
        "tokens": tokens,
        "mytokens_unread": mytokens,
    }))
}

/// Adds `a`, refused when another run kept its name meanwhile.
fn insert(store: &Store, a: &Account) -> Result<(), Refused> {
    if store.insert(a)? {
        return Ok(());
    }
    Err(Refused::new(
        Code::NameTaken,
        format!(
            "account `{}` was kept meanwhile: run the scan again",
            a.name
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_are_the_agent_account_lines_with_their_fields() {
        let list = "agent-account-codex/default\tprofile\tserver  — fields: as about  fields: HARNESS, MACHINE\n\
                    bark/default\tstatic\tserver  — push\n\
                    agent-account-/default\tprofile\tserver  — no name  fields: HARNESS\n\
                    agent-account-x/work\tprofile\tpersonal  — none";
        let all: Vec<_> = entries(list).collect();
        assert_eq!(
            all,
            [
                Entry {
                    id: "agent-account-codex/default",
                    service: "agent-account-codex",
                    label: "default",
                    fields: vec!["HARNESS", "MACHINE"],
                },
                Entry {
                    id: "agent-account-x/work",
                    service: "agent-account-x",
                    label: "work",
                    fields: vec![],
                },
            ]
        );
        assert_eq!(all[0].token(), None);
        assert_eq!(
            all[1].token_command("CLAUDE_CODE_OAUTH_TOKEN"),
            "mytokens get agent-account-x --account 'work' --field CLAUDE_CODE_OAUTH_TOKEN"
        );
    }

    #[test]
    fn an_identity_is_hashed_and_a_jwt_s_claims_are_read() {
        assert_eq!(hash("chatgpt:a"), hash("chatgpt:a"));
        assert_ne!(hash("chatgpt:a"), hash("kimi:a"));
        assert_eq!(hash("x").len(), 16);
        let claims = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(r#"{"user_id":"u"}"#);
        assert_eq!(jwt(&format!("h.{claims}.s")).unwrap()["user_id"], "u");
        assert!(jwt("no-dots").is_err());
        assert!(jwt("h.!!!.s").is_err());
    }

    fn at(engine: Engine, home: &str, written: u64, id: Result<&str, &str>) -> Found {
        Found {
            engine,
            home: PathBuf::from(home),
            written,
            identity: id.map(String::from).map_err(String::from),
        }
    }

    #[test]
    fn one_account_per_identity_the_newest_or_the_kept_one() {
        use Engine::*;
        let found = [
            at(Codex, "/h/.codex", 10, Ok("chatgpt")),
            at(Pi, "/h/.pi/agent", 20, Ok("chatgpt")),
            at(ClaudeCode, "/h/.claude", 5, Ok("claude")),
            at(ClaudeCode, "/x/claude", 1, Ok("claude")),
            at(Kimi, "/h/.kimi-code", 3, Err("unknown")),
            at(Grok, "/h/.grok", 3, Ok("grok")),
        ];
        let bound = |name: &str, engine, home: &str| {
            let terms = Terms {
                machine: Some("mini".into()),
                home: Some(home.into()),
                ..Terms::default()
            };
            let change = Change {
                terms,
                ..Change::default()
            };
            account::new(name, engine, change).unwrap()
        };
        let kept = [
            (bound("work", ClaudeCode, "/x/claude"), None),
            (bound("grok", Grok, "/elsewhere"), Some("other".into())),
        ];
        let mut taken = vec!["work".to_string(), "grok".to_string()];
        let judged = judge(&found, &kept, "mini", &mut taken);
        use Status::*;
        assert_eq!(
            judged,
            [
                Copy("pi".into()),
                New("pi".into()),
                Copy("work".into()),
                Registered("work".into()),
                New("kimi".into()),
                New("grok-2".into()),
            ]
        );
        // On another machine the kept home is not this one's.
        let mut taken = vec!["work".to_string(), "grok".to_string()];
        let judged = judge(&found, &kept, "studio", &mut taken);
        assert_eq!(
            judged[2..4],
            [New("claude-code".into()), Copy("claude-code".into())]
        );
    }
}
