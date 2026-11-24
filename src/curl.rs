//! HTTP through curl's command line: the one place that builds it. A
//! header (a bearer among them) goes to curl on its stdin, as its config,
//! never in its arguments, where `ps` would show it.
use std::io::Write;
use std::process::{Command, Stdio};

/// How long one request may take, in seconds.
const MAX_TIME: &str = "20";

/// GETs `url` with `headers` (`Name: value`): the HTTP status and the body,
/// or why curl got neither.
pub fn get(url: &str, headers: &[String]) -> Result<(u16, String), String> {
    let mut child = Command::new("curl")
        .args(["--silent", "--show-error", "--max-time", MAX_TIME])
        .args(["--config", "-", "--write-out", "\n%{http_code}", url])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("curl: {e}: install curl, or put it on PATH"))?;
    let config: String = headers
        .iter()
        .map(|h| {
            format!(
                "header = \"{}\"\n",
                h.replace('\\', "\\\\").replace('"', "\\\"")
            )
        })
        .collect();
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(config.as_bytes())
            .map_err(|e| format!("curl: {e}"))?;
    }
    let out = child.wait_with_output().map_err(|e| format!("curl: {e}"))?;
    if !out.status.success() {
        let said = String::from_utf8_lossy(&out.stderr);
        return Err(format!("{url}: {}", said.trim()));
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let (body, code) = stdout.rsplit_once('\n').unwrap_or(("", &stdout));
    let code = code
        .trim()
        .parse()
        .map_err(|_| format!("{url}: curl answered no HTTP status"))?;
    Ok((code, body.to_string()))
}
