//! dsh's reader: `GET https://platform.deepseek.com/api/v0/users/get_user_summary`
//! with the account's grant in `x-dsh-auth-token` and dsh's own client
//! headers. A DeepSeek account is pay-as-you-go: its reading is a
//! **balance** per wallet (`biz_data.normal_wallets[]`, `bonus_wallets[]`),
//! no window. dsh drops its grant on HTTP 401 or code 40003, so this
//! reader reports either as a refused credential, not asked again until
//! `stablehand account login`.
use crate::account::Account;
use crate::books::Balance;
use crate::readers::Fault;
use crate::{curl, login};
use std::path::PathBuf;

const SUMMARY: &str = "https://platform.deepseek.com/api/v0/users/get_user_summary";

/// dsh's five client headers, as it sends them from a terminal (its
/// `platformClientHeaders`, dsh 0.2.2): the bundle id empty (curl sends a
/// header named with `;` empty), the platform `web`, its version, the
/// locale, the UTC offset in seconds.
const CLIENT: &[&str] = &[
    "x-client-bundle-id;",
    "x-client-platform: web",
    "x-client-version: 0.2.2-rc.1",
    "x-client-locale: en_US",
    "x-client-timezone-offset: 0",
];

/// The record of dsh's credential store that holds its Platform grant.
const GRANT_RECORD: &str = "deepseek-account-platform/default:";

pub fn read(account: &Account) -> Result<Vec<Balance>, Fault> {
    let grant = grant(account).map_err(Fault::Failed)?;
    let mut headers = vec![format!("x-dsh-auth-token: {grant}")];
    headers.extend(CLIENT.iter().map(|h| h.to_string()));
    let (status, body) = curl::get(SUMMARY, &headers).map_err(Fault::Failed)?;
    balances(status, &body, &account.name)
}

/// The account's grant: a portable login's from sesame (as `stablehand
/// account login --token` keeps it), a bound one's from dsh's credential
/// store in its home, `<DSH_HOME>/.credentials.yaml` (`~/.dsh` where it
/// names none).
fn grant(account: &Account) -> Result<String, String> {
    let name = &account.name;
    if account.machine.is_none() {
        return login::token(account)?.ok_or_else(|| {
            format!("sesame keeps no token for `{name}`: log it in with `stablehand account login {name} --token`")
        });
    }
    let home = match &account.home {
        Some(home) => PathBuf::from(home),
        None => PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".dsh"),
    };
    let file = home.join(".credentials.yaml");
    let fix = |why: &str| {
        format!(
            "{}: {why}: log it in on {} with `stablehand account login {name}`",
            file.display(),
            account.machine.as_deref().unwrap_or_default()
        )
    };
    let text = std::fs::read_to_string(&file).map_err(|e| fix(&e.to_string()))?;
    grant_in(&text).ok_or_else(|| fix("no deepseek-account-platform grant in it"))
}

/// The `token` of the grant record in dsh's credential store, `text`: the
/// first `token:` indented under [`GRANT_RECORD`].
fn grant_in(text: &str) -> Option<String> {
    let indent = |l: &str| l.len() - l.trim_start().len();
    let mut lines = text.lines();
    let record = lines.find(|l| l.trim() == GRANT_RECORD)?;
    let under = indent(record);
    lines
        .take_while(|l| l.trim().is_empty() || indent(l) > under)
        .find_map(|l| l.trim().strip_prefix("token:"))
        .map(|t| t.trim().trim_matches(['"', '\'']).to_string())
        .filter(|t| !t.is_empty())
}

/// The balances of the endpoint's answer, `status` and `body`, or why it
/// gave none, with the fix.
fn balances(status: u16, body: &str, name: &str) -> Result<Vec<Balance>, Fault> {
    let answer: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
    let said = answer["msg"].as_str().unwrap_or("no message");
    if status == 401 || answer["code"] == 40003 {
        return Err(Fault::Refused(format!(
            "{SUMMARY} refused the login (HTTP {status}, code {}: {said}); dsh drops it too: log in again with `stablehand account login {name}`",
            answer["code"]
        )));
    }
    let later = "try again later (`stablehand read --now`)";
    if status != 200 {
        return Err(Fault::Failed(format!(
            "{SUMMARY} answered HTTP {status}: {said}: {later}"
        )));
    }
    let data = &answer["data"];
    if answer["code"] != 0 || data["biz_code"] != 0 {
        let biz = data["biz_msg"].as_str().unwrap_or("");
        return Err(Fault::Failed(format!(
            "{SUMMARY} answered code {} ({said}), biz_code {} ({biz}): {later}",
            answer["code"], data["biz_code"]
        )));
    }
    let found: Vec<Balance> = [("normal", "normal_wallets"), ("bonus", "bonus_wallets")]
        .into_iter()
        .flat_map(|(wallet, key)| {
            data["biz_data"][key]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(move |w| {
                    Some(Balance {
                        wallet: wallet.to_string(),
                        currency: w["currency"].as_str()?.to_string(),
                        amount: w["balance"].as_str()?.parse().ok()?,
                    })
                })
        })
        .collect();
    if found.is_empty() {
        return Err(Fault::Failed(format!(
            "{SUMMARY} answered no wallet: its answer has changed; run `truefit contract --real dsh` in stablehand's repo"
        )));
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grant_is_the_platform_record_s_token() {
        let store = "version: 1\nrecords:\n  deepseek-account-platform/device:\n    kind: grant\n    payload:\n      id: 448a\n      token: not-this\n  deepseek-account-platform/default:\n    kind: grant\n    payload:\n      version: 1\n      token: dsh-standin-grant\n      issuer: https://platform.deepseek.com\n";
        assert_eq!(grant_in(store).as_deref(), Some("dsh-standin-grant"));
        let quoted =
            "records:\n  deepseek-account-platform/default:\n    payload:\n      token: 'q'\n";
        assert_eq!(grant_in(quoted).as_deref(), Some("q"));
        let none = "records:\n  deepseek-account-platform/default:\n    kind: grant\n  other:\n    token: x\n";
        assert_eq!(grant_in(none), None);
        assert_eq!(grant_in("records: {}\n"), None);
    }

    #[test]
    fn the_answer_s_wallets_are_its_balances() {
        let body = r#"{"code":0,"msg":"","data":{"biz_code":0,"biz_msg":"","biz_data":{
            "normal_wallets":[{"currency":"CNY","balance":"94.6700242600000000","token_estimation":"1"}],
            "bonus_wallets":[{"currency":"CNY","balance":"0","token_estimation":"0"},{"currency":"USD","balance":"x"}],
            "total_costs":[{"currency":"CNY","amount":"5.33"}]}}}"#;
        let got = balances(200, body, "d").unwrap();
        let shown: Vec<_> = got
            .iter()
            .map(|b| (b.wallet.as_str(), b.currency.as_str(), b.amount))
            .collect();
        assert_eq!(
            shown,
            [("normal", "CNY", 94.67002426), ("bonus", "CNY", 0.0)]
        );
    }

    #[test]
    fn a_refused_login_is_refused_and_other_faults_fail() {
        let refused = r#"{"code":40003,"msg":"Authorization Failed (invalid token)","data":null}"#;
        let Err(Fault::Refused(why)) = balances(200, refused, "deepseek") else {
            panic!("40003 is a refusal")
        };
        assert!(
            why.contains("invalid token") && why.contains("stablehand account login deepseek"),
            "{why}"
        );
        assert!(matches!(balances(401, "", "d"), Err(Fault::Refused(_))));
        let Err(Fault::Failed(why)) = balances(502, "bad gateway", "d") else {
            panic!("a 502 fails")
        };
        assert!(
            why.contains("HTTP 502: no message: try again later"),
            "{why}"
        );
        let missing = r#"{"code":40002,"msg":"Missing Token","data":null}"#;
        let Err(Fault::Failed(why)) = balances(200, missing, "d") else {
            panic!("40002 fails")
        };
        assert!(why.contains("code 40002 (Missing Token)"), "{why}");
        let biz = r#"{"code":0,"msg":"","data":{"biz_code":7,"biz_msg":"busy","biz_data":null}}"#;
        let Err(Fault::Failed(why)) = balances(200, biz, "d") else {
            panic!("a biz_code fails")
        };
        assert!(why.contains("biz_code 7 (busy)"), "{why}");
        let empty = r#"{"code":0,"msg":"","data":{"biz_code":0,"biz_data":{"normal_wallets":[]}}}"#;
        let Err(Fault::Failed(why)) = balances(200, empty, "d") else {
            panic!("no wallet fails")
        };
        assert!(why.contains("answered no wallet"), "{why}");
    }
}
