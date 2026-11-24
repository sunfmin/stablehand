//! A register of a test's own, run through the binary: its config dir, and a
//! PATH whose `sesame`, `mytokens` and engines are the stand-ins (tests/contract,
//! tests/engines), each appending its argv to `argv.log`.
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
        link("contract/fake-sesame", "sesame").unwrap();
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

    /// The token sesame's stand-in keeps for `provider`.
    pub fn sesame_token(&self, provider: &str) -> Option<String> {
        std::fs::read_to_string(self.root.join("sesame/fake/tokens").join(provider)).ok()
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
            .env("SESAME_CONFIG_DIR", self.root.join("sesame"))
            .env("FAKE_MYTOKENS_DIR", self.root.join("mytokens"))
            .env("FAKE_ARGV_LOG", self.root.join("argv.log"))
            .env("STABLEHAND_MACHINE", "mini")
            .envs(env.iter().copied())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
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
