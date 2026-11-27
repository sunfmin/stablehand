//! `account scan`: the engine logins already on this machine, and the
//! portable tokens optokens keeps, found and, with `--add`, registered: one
//! account per subscription, told apart by the identity each login file
//! holds, a portable Claude token by the organization its answers name. An
//! identity is read and compared in memory, and only its hash ([`hash`]) is
//! answered or kept; a token is read only to ask its organization, and
//! never said or kept (docs/adr/0001).
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
    /// A Claude login's organization.
    organization: Option<Org>,
}

/// A Claude organization as scan compares it: its id's hash ([`org_hash`]),
/// and its `organizationType`.
#[derive(Debug, Clone, PartialEq)]
struct Org {
    hash: String,
    kind: Option<String>,
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
pub const IDENTITIES: &str = "claude-code: its account and organization in `.claude.json` (its home's, or `~/.claude.json` for `~/.claude`), and the organization's type (`organizationType`); a portable claude-code token: the organization its answers name, read by one `POST https://api.anthropic.com/v1/messages` of an empty request, which is refused before any turn, so nothing is spent; codex: `tokens.account_id`, the ChatGPT account; pi: `openai-codex.accountId`, the same ChatGPT account; kimi: its access token's `user_id`; agy: `id_token_email`; grok: its login's `user_id`; dsh: none, its file holds only a grant";

/// The identity `engine`'s login in `home` holds, `<kind>:<id>` (codex's and
/// pi's one kind: a ChatGPT account), with a Claude login's organization, or
/// why it cannot be read. Never said or kept: [`hash`] of it is.
fn identity(engine: Engine, home: &Path) -> Result<(String, Option<Org>), String> {
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
            let kind = v.pointer("/oauthAccount/organizationType");
            let organization = Org {
                hash: org_hash(&org),
                kind: kind.and_then(Value::as_str).map(String::from),
            };
            return Ok((format!("claude:{user}/{org}"), Some(organization)));
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
    Ok((format!("{kind}:{id}"), None))
}

/// A Claude organization's id as it is said and kept: its [`hash`], the
/// identity of a portable token of it.
fn org_hash(org: &str) -> String {
    hash(&format!("claude-org:{org}"))
}

/// The hash of the organization the token `account`'s command prints
/// names, or why it was not read.
fn token_org(account: &Account) -> Result<String, String> {
    let bearer = login::token(account)?;
    crate::readers::claude_code::organization(&bearer).map(|o| org_hash(&o))
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
pub fn same(a: &Path, b: &Path) -> bool {
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
        let read = identity(engine, &home);
        Some(Found {
            engine,
            home,
            written,
            identity: read.as_ref().map(|(i, _)| hash(i)).map_err(String::clone),
            organization: read.ok().and_then(|(_, o)| o),
        })
    };
    let mut all: Vec<Found> = engines
        .iter()
        .filter_map(|&e| match e.login().default_home {
            "" => found(e, user_home.to_path_buf()),
            rel => found(e, user_home.join(rel)),
        })
        .collect();
    for path in paths {
        let mut here: Vec<_> = engines
            .iter()
            .filter_map(|&e| found(e, path.clone()))
            .collect();
        if here.iter().any(|f| f.identity.is_ok()) {
            here.retain(|f| f.identity.is_ok());
        }
        here.retain(|h| {
            !all.iter()
                .any(|f| f.engine == h.engine && same(&f.home, path))
        });
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
/// the hash of its identity, kept or read in its home) and bound to `machine`:
/// an account kept at its home is it; then, newest first, one whose
/// identity an account has is a copy of that, and the rest are new, the
/// newest of an identity its account and the others copies of it. A login
/// whose identity cannot be read is new, never a copy.
fn judge(found: &[Found], kept: &[Account], machine: &str, taken: &mut Vec<String>) -> Vec<Status> {
    let mut owner: HashMap<String, String> = HashMap::new();
    let mut status: Vec<Option<Status>> = found
        .iter()
        .map(|f| {
            let a = kept.iter().find(|a| {
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
    for a in kept {
        if let Some(id) = &a.identity {
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
        let id = f.identity.as_ref().ok();
        status[i] = Some(match id.and_then(|id| owner.get(id)) {
            Some(name) => Status::Copy(name.clone()),
            None => {
                let name = fresh(taken, &f.engine.name());
                if let Some(id) = id {
                    owner.insert(id.clone(), name.clone());
                }
                Status::New(name)
            }
        });
    }
    status
        .into_iter()
        .map(|s| s.expect("each judged"))
        .collect()
}

const PREFIX: &str = "agent-account-";

/// One `agent-account-*` Secret as `optokens list` lists it: its fields'
/// labels, `null` for a single value.
#[derive(serde::Deserialize)]
struct Entry {
    service: String,
    account: String,
    fields: Option<Vec<String>>,
}

/// The `agent-account-*` entries of `optokens list`'s JSON,
/// `{"secrets":[…]}`, or why it is not that JSON.
fn entries(list: &str) -> Result<Vec<Entry>, String> {
    #[derive(serde::Deserialize)]
    struct List {
        secrets: Vec<Entry>,
    }
    let mut list: List = serde_json::from_str(list)
        .map_err(|e| format!("`optokens list` answered no list of Secrets: {e}"))?;
    list.secrets.retain(|s| {
        s.service
            .strip_prefix(PREFIX)
            .is_some_and(|n| !n.is_empty())
    });
    Ok(list.secrets)
}

impl Entry {
    /// `<service>/<account>`.
    fn id(&self) -> String {
        format!("{}/{}", self.service, self.account)
    }

    fn has(&self, field: &str) -> bool {
        self.fields.iter().flatten().any(|f| f == field)
    }

    /// The engine whose token variable is one of its fields, and that field.
    fn token(&self) -> Option<(Engine, &'static str)> {
        let engines = <Engine as clap::ValueEnum>::value_variants();
        engines.iter().find_map(|&e| {
            let var = e.login().token_var?;
            self.has(var).then_some((e, var))
        })
    }

    /// The `optokens get` that prints its token field `var`.
    fn token_command(&self, var: &str) -> String {
        let account = match self.account.as_str() {
            "default" => String::new(),
            label => format!(" --account '{}'", label.replace('\'', "'\\''")),
        };
        format!("optokens get {}{account} --field {var}", self.service)
    }
}

/// The `agent-account-*` entries of `optokens list`, or why it was not
/// read: optokens not on PATH, not started, or failing with the faults and
/// fixes it answers on stdout.
fn optokens_list() -> Result<Vec<Entry>, String> {
    let out = Command::new("optokens")
        .arg("list")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    match out {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err("not on PATH: its entries were not looked at".into())
        }
        Err(e) => Err(format!("`optokens list` could not start: {e}")),
        Ok(o) if !o.status.success() => Err(failed(&o.stdout)),
        Ok(o) => entries(&String::from_utf8_lossy(&o.stdout)),
    }
}

/// Why `optokens list` failed: each `{fault, fix}` of the
/// `{"error":[…]}` it answers on stdout.
fn failed(stdout: &[u8]) -> String {
    let said: Vec<String> = serde_json::from_slice::<Value>(stdout)
        .ok()
        .iter()
        .flat_map(|v| v["error"].as_array().into_iter().flatten())
        .filter_map(|e| Some(format!("{} ({})", e["fault"].as_str()?, e["fix"].as_str()?)))
        .collect();
    match said.is_empty() {
        true => "`optokens list` failed and said no fault".into(),
        false => format!("`optokens list` failed: {}", said.join("; ")),
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
    let stored = store.list()?;
    // Each kept account with the hashes read in its home on this machine
    // where the register keeps none; a token's are read below.
    let mut kept: Vec<Account> = stored
        .iter()
        .map(|a| {
            let read = match (&a.machine, &a.home) {
                (Some(m), Some(h)) if *m == machine => identity(a.engine, Path::new(h)).ok(),
                _ => None,
            };
            let (id, org) = read.map_or((None, None), |(i, o)| (Some(hash(&i)), o));
            let mut a = a.clone();
            a.identity = a.identity.or(id);
            if a.organization.is_none() {
                a.organization = org.as_ref().map(|o| o.hash.clone());
                a.organization_type = org.and_then(|o| o.kind);
            }
            a
        })
        .collect();
    let mut taken: Vec<String> = kept.iter().map(|a| a.name.clone()).collect();
    let judged = judge(&found, &kept, &machine, &mut taken);

    let mut out_logins = Vec::new();
    for (f, status) in found.iter().zip(&judged) {
        let (status, name) = match status {
            Status::New(n) => {
                let terms = Terms {
                    machine: Some(machine.clone()),
                    home: Some(f.home.display().to_string()),
                    ..Terms::default()
                };
                let mut a = new(n, f.engine, terms)?;
                a.identity = f.identity.clone().ok();
                a.organization = f.organization.as_ref().map(|o| o.hash.clone());
                a.organization_type = f.organization.as_ref().and_then(|o| o.kind.clone());
                let status = register(store, add, &a)?;
                // Beside the kept ones, what a token may be a copy of.
                kept.push(a);
                (status, n)
            }
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
    let (list, unread) = match optokens_list() {
        Ok(list) => (list, None),
        Err(why) => (Vec::new(), Some(why)),
    };
    // Each entry in its order: a skipped one as answered, a token as read.
    let mut rows: Vec<Result<Token, Value>> = Vec::new();
    for e in &list {
        let Some((engine, var)) = e.token() else {
            let why = match e.has("MACHINE") {
                true => {
                    "bound: its login is in a home of factorium's (~/.agent-accounts), which scan does not look in; `--path` that home to find it"
                }
                false => "it holds no engine's token field",
            };
            rows.push(Err(
                json!({ "entry": e.id(), "status": "skipped", "why": why }),
            ));
            continue;
        };
        let cmd = e.token_command(var);
        let legacy = format!(
            "optokens get {} --account {} --field {var}",
            e.service, e.account
        );
        let kept_at = kept.iter().position(|a| {
            a.machine.is_none()
                && a.token_command
                    .as_ref()
                    .is_some_and(|c| *c == cmd || *c == legacy)
        });
        let mut account = match kept_at {
            Some(i) => kept[i].clone(),
            None => {
                let terms = Terms {
                    token_command: Some(cmd.clone()),
                    ..Terms::default()
                };
                let name = fresh(&mut taken, &e.service[PREFIX.len()..]);
                new(&name, engine, terms)?
            }
        };
        let org = match engine {
            Engine::ClaudeCode => token_org(&account),
            _ => Err(format!("a {} token tells none", engine.name())),
        };
        account.identity = account.identity.or(org.clone().ok());
        if let Some(i) = kept_at {
            kept[i].identity.clone_from(&account.identity);
        }
        rows.push(Ok(Token {
            entry: e.id(),
            command: cmd,
            kept: kept_at.is_some(),
            account,
            org,
        }));
    }
    let mut tokens = Vec::new();
    for row in &rows {
        let t = match row {
            Ok(t) => t,
            Err(skipped) => {
                tokens.push(skipped.clone());
                continue;
            }
        };
        let a = &t.account;
        let copy = account::copy_of(a, &kept);
        let status = match (t.kept, copy) {
            (true, _) => "registered",
            (false, Some(_)) => "copy",
            (false, None) => register(store, add, a)?,
        };
        let warning = match copy {
            Some(login) => Some(format!(
                "a token of {login}'s subscription (its organization is {login}'s, one person's): both spend the same windows, so run sessions on one of them only and switch the other off (`stablehand off <account>`)"
            )),
            None => {
                let entries = of_its_org(&rows, t);
                (!entries.is_empty()).then(|| format!(
                    "of the organization of the token of {} too: one subscription if it is one person's, a team's seats if not, which a token does not tell; if one person's, run sessions on one of them only",
                    entries.join(", ")
                ))
            }
        };
        tokens.push(json!({
            "entry": t.entry,
            "engine": a.engine.name(),
            "token_command": t.command,
            "status": status,
            "account": copy.filter(|_| !t.kept).unwrap_or(&a.name),
            "identity": t.org.as_ref().ok(),
            "identity_unknown": t.org.as_ref().err(),
            "same_as_a_login": copy.unwrap_or("unknown"),
            "warning": warning,
        }));
    }
    if add {
        for (a, was) in kept.iter().zip(&stored) {
            if a != was {
                store.fill_hashes(a)?;
            }
        }
    }
    Ok(json!({
        "add": add,
        "machine": machine,
        "logins": out_logins,
        "tokens": tokens,
        "optokens_unread": unread,
    }))
}

/// The other tokens of `rows` of `t`'s organization, by entry.
fn of_its_org<'a>(rows: &'a [Result<Token, Value>], t: &Token) -> Vec<&'a str> {
    let Ok(id) = &t.org else {
        return Vec::new();
    };
    let same = |o: &&'a Token| o.entry != t.entry && o.org.as_ref() == Ok(id);
    rows.iter()
        .flatten()
        .filter(same)
        .map(|o| o.entry.as_str())
        .collect()
}

/// A token entry as scan read it.
struct Token {
    /// `<service>/<label>`.
    entry: String,
    command: String,
    /// Whether `account` is kept; else it is as `--add` would keep it.
    kept: bool,
    account: Account,
    /// Its organization's hash, or why it was not read.
    org: Result<String, String>,
}

/// A new account `name` of `engine` with `terms`.
fn new(name: &str, engine: Engine, terms: Terms) -> Result<Account, Refused> {
    let change = Change {
        terms,
        ..Change::default()
    };
    Ok(account::new(name, engine, change).map_err(|f| f.join("; "))?)
}

/// A `new` one's status: with `add`, `a` added; refused when another run
/// kept its name meanwhile.
fn register(store: &Store, add: bool, a: &Account) -> Result<&'static str, Refused> {
    if !add {
        return Ok("new");
    }
    if store.insert(a)? {
        return Ok("added");
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
    fn a_new_one_kept_meanwhile_by_another_run_is_refused_and_the_kept_one_stays() {
        let store = Store::in_memory();
        let home = |h: &str| Terms {
            machine: Some("m".into()),
            home: Some(h.into()),
            ..Terms::default()
        };
        let first = new("claude", Engine::ClaudeCode, home("/a")).unwrap();
        assert_eq!(register(&store, false, &first), Ok("new"));
        assert!(store.get("claude").unwrap().is_none());
        assert_eq!(register(&store, true, &first), Ok("added"));
        // Another run kept `claude` after this one listed the register.
        let later = new("claude", Engine::ClaudeCode, home("/b")).unwrap();
        let refused = register(&store, true, &later).unwrap_err();
        assert_eq!(refused.code, Code::NameTaken);
        assert_eq!(
            refused.text,
            "account `claude` was kept meanwhile: run the scan again"
        );
        assert_eq!(
            store.get("claude").unwrap().unwrap().home.as_deref(),
            Some("/a")
        );
    }

    #[test]
    fn entries_are_the_agent_account_secrets_with_their_fields() {
        let list = r#"{"secrets":[
            {"service":"agent-account-codex","account":"default","kind":"profile","description":"d","meta":null,"fields":["HARNESS","MACHINE"]},
            {"service":"bark","account":"default","kind":"static","description":"push","meta":null,"fields":null},
            {"service":"agent-account-","account":"default","kind":"profile","description":"no name","meta":null,"fields":["HARNESS"]},
            {"service":"agent-account-x","account":"work","kind":"profile","description":"none","meta":null,"fields":null}
        ]}"#;
        let all = entries(list).unwrap();
        let ids: Vec<_> = all.iter().map(|e| (e.id(), e.fields.clone())).collect();
        let fields = Some(vec!["HARNESS".to_string(), "MACHINE".to_string()]);
        assert_eq!(
            ids,
            [
                ("agent-account-codex/default".to_string(), fields),
                ("agent-account-x/work".to_string(), None),
            ]
        );
        assert_eq!(all[0].token(), None);
        assert_eq!(
            all[1].token_command("CLAUDE_CODE_OAUTH_TOKEN"),
            "optokens get agent-account-x --account 'work' --field CLAUDE_CODE_OAUTH_TOKEN"
        );
        assert!(entries("agent-account-x/default\tprofile").is_err());
    }

    #[test]
    fn a_failed_list_says_the_faults_it_answered() {
        let said = br#"{"error":[{"fault":"no token","fix":"sign in","kind":"no_token"}]}"#;
        assert_eq!(failed(said), "`optokens list` failed: no token (sign in)");
        assert_eq!(failed(b""), "`optokens list` failed and said no fault");
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
            organization: None,
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
            bound("work", ClaudeCode, "/x/claude"),
            Account {
                identity: Some("other".into()),
                ..bound("grok", Grok, "/elsewhere")
            },
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

    #[test]
    fn two_tokens_of_one_organization_are_said_so() {
        let token = |entry: &str, org: Result<&str, &str>| Token {
            entry: entry.into(),
            command: String::new(),
            kept: false,
            account: account::new("t", Engine::ClaudeCode, Change::default()).unwrap(),
            org: org.map(String::from).map_err(String::from),
        };
        let rows = [
            Ok(token("a", Ok("team"))),
            Ok(token("b", Ok("max"))),
            Err(Value::Null),
            Ok(token("c", Err("not read"))),
            Ok(token("d", Ok("team"))),
        ];
        let of: Vec<_> = rows
            .iter()
            .flatten()
            .map(|t| of_its_org(&rows, t))
            .collect();
        assert_eq!(of, [vec!["d"], vec![], vec![], vec!["a"]]);
        assert_ne!(org_hash("o"), hash("o"));
        assert_eq!(org_hash("o").len(), 16);
    }
}
