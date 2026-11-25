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
    run(url, headers, &[], "\n%{http_code}")
}

/// POSTs `data` to `url` with `headers`: the HTTP status and the value of
/// the answer's header `name`, empty where it has none (the body is not
/// read), or why curl got neither.
pub fn post_for_header(
    url: &str,
    headers: &[String],
    data: &str,
    name: &str,
) -> Result<(u16, String), String> {
    let write_out = format!("\n%header{{{name}}}\n%{{http_code}}");
    let (status, out) = run(url, headers, &["--data", data], &write_out)?;
    let value = out.rsplit_once('\n').map_or("", |(_, v)| v);
    Ok((status, value.trim().to_string()))
}

/// curl's answer for `url`, `headers` on its stdin, `args` and `write_out`
/// (which ends in a newline and the HTTP status) given: the status, and
/// what it printed before it.
fn run(
    url: &str,
    headers: &[String],
    args: &[&str],
    write_out: &str,
) -> Result<(u16, String), String> {
    let mut child = Command::new("curl")
        .args(["--silent", "--show-error", "--max-time", MAX_TIME])
        .args(args)
        .args(["--config", "-", "--write-out", write_out, url])
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
    let (before, code) = stdout.rsplit_once('\n').unwrap_or(("", &stdout));
    let code = code
        .trim()
        .parse()
        .map_err(|_| format!("{url}: curl answered no HTTP status"))?;
    Ok((code, before.to_string()))
}
