//! A register of a test's own, run through the binary: its config dir, and a
//! PATH whose `mytokens` and engines are the stand-ins (tests/contract,
//! tests/engines), each appending its argv to `argv.log`; stablehand's
//! stderr is kept in `stderr.log`.
#![allow(dead_code)]
use serde_json::Value;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

pub struct Register {
    pub dir: PathBuf,
    root: PathBuf,
}

pub struct Out {
    pub code: i32,
    pub json: Value,
}

impl Register {
    pub fn new(name: &str) -> Register {
        let root = std::env::temp_dir()
            .join(format!("stablehand-test-{}", std::process::id()))
            .join(name);
        let _ = std::fs::remove_dir_all(&root);
        let bin = root.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let tests = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");
        let link =
            |to: &str, name: &str| std::os::unix::fs::symlink(tests.join(to), bin.join(name));
        link("contract/fake-mytokens", "mytokens").unwrap();
        for engine in ["claude", "codex"] {
            link("engines/fake-engine", engine).unwrap();
        }
        Register {
            dir: root.join("config"),
            root,
        }
    }

    /// Where the test's own paths go (a bound login's home).
    pub fn path(&self, p: &str) -> String {
        self.root.join(p).display().to_string()
    }

    /// Each argv the stand-ins were run with, in order.
    pub fn argv(&self) -> String {
        std::fs::read_to_string(self.root.join("argv.log")).unwrap_or_default()
    }

    /// An entry `service`/default in mytokens' stand-in, with its fields.
    pub fn mytokens_entry(&self, service: &str, fields: &[(&str, &str)]) {
        let entry = self.root.join("mytokens").join(service).join("default");
        std::fs::create_dir_all(&entry).unwrap();
        for (field, value) in fields {
            std::fs::write(entry.join(field), value).unwrap();
        }
    }

    /// `token` kept in mytokens' stand-in as `service`'s
    /// CLAUDE_CODE_OAUTH_TOKEN, and the token command that prints it.
    pub fn kept_token(&self, service: &str, token: &str) -> String {
        self.mytokens_entry(service, &[("CLAUDE_CODE_OAUTH_TOKEN", token)]);
        format!("mytokens get {service} --account default --field CLAUDE_CODE_OAUTH_TOKEN")
    }

    /// What stablehand wrote to its stderr, every run of it.
    pub fn stderr(&self) -> String {
        std::fs::read_to_string(self.root.join("stderr.log")).unwrap_or_default()
    }

    /// Every file under the test's own paths whose bytes hold `secret`.
    pub fn holding(&self, secret: &str) -> Vec<String> {
        holding(&self.root, secret)
    }

    /// The binary run on `args`, `stdin` piped in, with `env` beside the test's own:
    /// its exit code and stdout as it printed it.
    pub fn raw(&self, args: &[&str], env: &[(&str, &str)], stdin: &str) -> (i32, String) {
        let mut child = Command::new(env!("CARGO_BIN_EXE_stablehand"))
            .args(args)
            .env("STABLEHAND_CONFIG_DIR", &self.dir)
            .env("USER", "tester")
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", self.root.join("bin").display()),
            )
            .env("FAKE_MYTOKENS_DIR", self.root.join("mytokens"))
            .env("FAKE_ARGV_LOG", self.root.join("argv.log"))
            .env("STABLEHAND_MACHINE", "mini")
            .envs(env.iter().copied())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(stderr_log(&self.root))
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(stdin.as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        let stdout = String::from_utf8(out.stdout).unwrap();
        (out.status.code().unwrap(), stdout)
    }

    /// The binary run as [`Register::raw`] runs it, its stdout one JSON line.
    pub fn run_with(&self, args: &[&str], env: &[(&str, &str)], stdin: &str) -> Out {
        let (code, stdout) = self.raw(args, env, stdin);
        assert_eq!(stdout.lines().count(), 1, "one JSON line: {stdout}");
        Out {
            code,
            json: serde_json::from_str(&stdout).unwrap(),
        }
    }

    pub fn run(&self, args: &[&str]) -> Out {
        self.run_with(args, &[], "")
    }

    pub fn ok(&self, args: &[&str]) -> Value {
        let out = self.run(args);
        assert_eq!(out.code, 0, "{args:?}: {}", out.json);
        out.json
    }

    /// The refusal's message, after checking it is one and wrote nothing.
    pub fn refused(&self, args: &[&str], code: i32) -> String {
        self.refused_with(args, &[], "", code)
    }

    pub fn refused_with(
        &self,
        args: &[&str],
        env: &[(&str, &str)],
        stdin: &str,
        code: i32,
    ) -> String {
        let before = self.ok(&["account", "list"]);
        let out = self.run_with(args, env, stdin);
        assert_eq!(out.code, code, "{args:?}: {}", out.json);
        assert_eq!(self.ok(&["account", "list"]), before, "{args:?} wrote");
        out.json["error"].as_str().unwrap().to_string()
    }
}

/// `stderr.log` under `root`, opened to append.
fn stderr_log(root: &std::path::Path) -> std::fs::File {
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join("stderr.log"))
        .unwrap()
}

/// Every file under `dir` whose bytes hold `secret`.
fn holding(dir: &std::path::Path, secret: &str) -> Vec<String> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            found.extend(holding(&path, secret));
        } else if let Ok(bytes) = std::fs::read(&path)
            && bytes.windows(secret.len()).any(|w| w == secret.as_bytes())
        {
            found.push(path.display().to_string());
        }
    }
    found
}

impl Register {
    /// A PATH whose engine programs (claude, codex, agy, …) are the session
    /// stand-in (tests/engines/fake-session), before the test's own bin.
    pub fn session_path(&self) -> String {
        let bin = self.root.join("session-bin");
        if !bin.exists() {
            std::fs::create_dir_all(&bin).unwrap();
            let stand_in =
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/engines/fake-session");
            for p in ["claude", "codex", "pi", "kimi", "agy", "grok", "dsh"] {
                std::os::unix::fs::symlink(&stand_in, bin.join(p)).unwrap();
            }
        }
        format!(
            "{}:{}:/usr/bin:/bin",
            bin.display(),
            self.root.join("bin").display()
        )
    }

    /// The binary started on `args` with `env` as [`Register::raw`] runs it,
    /// not waited for; its stdout piped.
    pub fn spawn(&self, args: &[&str], env: &[(&str, &str)]) -> std::process::Child {
        Command::new(env!("CARGO_BIN_EXE_stablehand"))
            .args(args)
            .env("STABLEHAND_CONFIG_DIR", &self.dir)
            .env("USER", "tester")
            .env("FAKE_MYTOKENS_DIR", self.root.join("mytokens"))
            .env("FAKE_ARGV_LOG", self.root.join("argv.log"))
            .env("STABLEHAND_MACHINE", "mini")
            .env("PATH", self.session_path())
            .envs(env.iter().copied())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(stderr_log(&self.root))
            .spawn()
            .unwrap()
    }
}
