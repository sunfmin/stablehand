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
    let out = run(url, headers, &[], "\n%{http_code}")?;
    let (body, code) = out.rsplit_once('\n').unwrap_or(("", &out));
    Ok((status(url, code)?, body.to_string()))
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
    let out = run(url, headers, &["--data", data], &write_out)?;
    let mut lines = out.rsplitn(3, '\n');
    let code = lines.next().unwrap_or_default();
    let value = lines.next().unwrap_or_default();
    Ok((status(url, code)?, value.trim().to_string()))
}

fn status(url: &str, code: &str) -> Result<u16, String> {
    code.trim()
        .parse()
        .map_err(|_| format!("{url}: curl answered no HTTP status"))
}

/// curl's stdout for `url`, `headers` on its stdin, `args` and `write_out` given.
fn run(url: &str, headers: &[String], args: &[&str], write_out: &str) -> Result<String, String> {
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
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}
