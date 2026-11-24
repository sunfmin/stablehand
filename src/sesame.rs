//! sesame's command line, where a portable login's token is kept: the one
//! place that builds it. The token is handed on, never kept or shown.
use std::process::Command;

/// The access token sesame keeps for `provider`, or what sesame said.
pub fn token(provider: &str) -> Result<String, String> {
    let out = Command::new("sesame")
        .args(["token", provider, "--json"])
        .output()
        .map_err(|e| {
            format!(
                "sesame: {e}: install it (`brew install sunfmin/tap/sesame`), or put it on PATH"
            )
        })?;
    if !out.status.success() {
        let said = String::from_utf8_lossy(&out.stderr);
        return Err(format!(
            "{}: a portable login's token is kept in sesame as `{provider}`: store it with `sesame login {provider} --token`",
            said.trim()
        ));
    }
    let answer: serde_json::Value = serde_json::from_slice(&out.stdout)
        .map_err(|e| format!("sesame token {provider} --json answered no JSON: {e}"))?;
    answer["access_token"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| format!("sesame token {provider} --json answered no access_token"))
}
