//! Muse Code: the session and weekly limits Meta reports for a Meta sign-in.

use reqwest::Client;
use serde_json::{json, Value};

use crate::http;
use crate::model::{now_ms, Credential, MetricLine, ProviderError, Result, Usage, SESSION_MS, WEEK_MS};

/// The Muse CLI's public client. The sign-in takes no scopes and has no renewal.
pub const CLIENT_ID: &str = "1031625952748946";
pub const DEVICE_CODE_URL: &str = "https://auth.meta.com/oidc/device/authorization/";
pub const TOKEN_URL: &str = "https://auth.meta.com/oidc/device/token/";
const ACCOUNT_URL: &str = "https://api.meta.ai/muse-code/key";

/// The account as Meta reports it. The answer also carries the account's model API key, which is
/// never read, stored or logged: only the fields named in this file are used.
pub async fn account(http: &Client, token: &str) -> Result<Value> {
    let request = http
        .post(ACCOUNT_URL)
        .bearer_auth(token)
        .header("Accept", "application/json")
        .header("x-api-version", "1.0.0")
        .json(&json!({"onboard": false}));
    http::send(request).await?.require_json(false)
}

pub async fn usage(http: &Client, credential: &Credential) -> Result<Usage> {
    map_account(&account(http, credential.secret()).await?, now_ms())
}

pub fn map_account(account: &Value, now: i64) -> Result<Usage> {
    if account["is_subs_active"].as_bool() != Some(true) {
        return Err(ProviderError::Unavailable("No Muse Code subscription on this account.".into()));
    }
    let usage = &account["subs_usage"];
    let lines = [("window", "Session", SESSION_MS), ("weekly", "Weekly", WEEK_MS)]
        .iter()
        .map(|(key, label, period)| {
            let window = &usage[*key];
            let resets_at = http::date_ms(&window["resets_at"]);
            // Muse leaves a window out while it is idle, and a window whose reset has passed is empty.
            let used = match http::num(&window["used_percent"]) {
                Some(used) if resets_at.map_or(true, |at| at > now) => used,
                _ => 0.0,
            };
            MetricLine::percent(label, used, resets_at.filter(|at| *at > now), Some(*period))
        })
        .collect();
    Ok(Usage { plan: http::text(&account["subs_tier_name"]), lines })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_both_windows_and_empties_a_passed_one() {
        let usage = map_account(
            &json!({"is_subs_active": true, "subs_tier_name": "Pro", "subs_usage": {
                "window": {"used_percent": 40, "resets_at": 2000},
                "weekly": {"used_percent": 90, "resets_at": 500}
            }}),
            1_000_000,
        )
        .unwrap();
        assert_eq!(usage.plan.as_deref(), Some("Pro"));
        assert_eq!(usage.lines[0], MetricLine::percent("Session", 40.0, Some(2_000_000), Some(SESSION_MS)));
        assert_eq!(usage.lines[1], MetricLine::percent("Weekly", 0.0, None, Some(WEEK_MS)));
    }

    #[test]
    fn an_idle_plan_reads_zero_and_no_plan_says_so() {
        let idle = map_account(&json!({"is_subs_active": true}), 0).unwrap();
        assert_eq!(idle.lines.iter().map(|line| line.used_percent()).collect::<Vec<_>>(), [Some(0.0), Some(0.0)]);
        assert!(matches!(map_account(&json!({"is_subs_active": false}), 0), Err(ProviderError::Unavailable(_))));
    }
}
