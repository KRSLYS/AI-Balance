//! OpenCode: the Go plan's caps for an OpenCode Console sign-in.

use reqwest::Client;
use serde_json::Value;

use super::Grant;
use crate::http::{self, Resp};
use crate::model::{Credential, MetricLine, ProviderError, Result, Usage, MONTH_MS, SESSION_MS, WEEK_MS};

/// The OpenCode CLI's public client.
pub const CLIENT_ID: &str = "opencode-cli";
pub const DEVICE_CODE_URL: &str = "https://opencode.ai/console/auth/device/code";
pub const TOKEN_URL: &str = "https://opencode.ai/console/auth/device/token";
pub const USER_URL: &str = "https://opencode.ai/console/api/user";
/// The workspace the sign-in was scoped to. Every read names it.
pub const ORG_KEY: &str = "org_id";
const USAGE_URL: &str = "https://opencode.ai/zen/go/v1/usage";
const STATUS_URL: &str = "https://opencode.ai/console/api/go/status";

pub async fn refresh(http: &Client, refresh_token: &str) -> Result<Grant> {
    let form = [("client_id", CLIENT_ID), ("grant_type", "refresh_token"), ("refresh_token", refresh_token)];
    Grant::from_renewal(&http::send(http.post(TOKEN_URL).form(&form)).await?)
}

pub async fn usage(http: &Client, credential: &Credential) -> Result<Usage> {
    let org = credential.extra(ORG_KEY).unwrap_or_default();
    let get =
        |url: &str, header: &str| http.get(url).bearer_auth(credential.secret()).header("Accept", "application/json").header(header, org);
    let lines = map_usage(&http::send(get(USAGE_URL, "x-opencode-org-id")).await?)?;
    // The plan name is a nicety: a failed read leaves the card without one.
    let plan = match http::send(get(STATUS_URL, "x-org-id")).await {
        Ok(response) if response.is_ok() => response.json().and_then(|body| plan_name(&body)),
        _ => None,
    };
    Ok(Usage { plan, lines })
}

pub fn map_usage(response: &Resp) -> Result<Vec<MetricLine>> {
    let body = response.require_json(false)?;
    let usage = &body["usage"];
    if !usage.is_object() {
        return Err(ProviderError::InvalidResponse);
    }
    let lines: Vec<MetricLine> = [("rolling", "Session", SESSION_MS), ("weekly", "Weekly", WEEK_MS), ("monthly", "Monthly", MONTH_MS)]
        .iter()
        .filter_map(|(key, label, period)| {
            let window = &usage[*key];
            Some(MetricLine::percent(label, http::num(&window["percent"])?, http::date_ms(&window["resetsAt"]), Some(*period)))
        })
        .collect();
    if lines.is_empty() {
        return Err(ProviderError::InvalidResponse);
    }
    Ok(lines)
}

pub fn plan_name(status: &Value) -> Option<String> {
    Some(
        match status["product"].as_str()? {
            "go" => "Go",
            "go-plus" => "Go Plus",
            "go-teams" => "Go Teams",
            _ => return None,
        }
        .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::resp;
    use serde_json::json;

    #[test]
    fn maps_the_three_caps() {
        let lines = map_usage(&resp(
            200,
            json!({"usage": {
                "rolling": {"percent": 12, "resetsAt": "2026-01-02T03:04:05Z"},
                "weekly": {"percent": 150, "resetsAt": "2026-01-08T00:00:00Z"},
                "monthly": {"percent": 3.5, "resetsAt": "2026-02-01T00:00:00Z"}
            }}),
        ))
        .unwrap();
        assert_eq!(lines[0], MetricLine::percent("Session", 12.0, Some(1_767_323_045_000), Some(SESSION_MS)));
        assert_eq!(lines[1].used_percent(), Some(100.0));
        assert_eq!(lines[2].label(), "Monthly");
    }

    #[test]
    fn plan_names_and_bad_answers() {
        assert_eq!(plan_name(&json!({"product": "go-plus"})).as_deref(), Some("Go Plus"));
        assert_eq!(plan_name(&json!({"product": "other"})), None);
        assert!(matches!(map_usage(&resp(200, json!({}))), Err(ProviderError::InvalidResponse)));
        assert!(matches!(map_usage(&resp(401, json!({}))), Err(ProviderError::SignInExpired)));
    }
}
