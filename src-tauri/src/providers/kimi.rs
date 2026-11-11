//! Kimi: the Kimi Code plan quotas Moonshot reports for a Kimi sign-in.

use reqwest::Client;
use serde_json::Value;

use super::Grant;
use crate::http::{self, Resp};
use crate::model::{Credential, MetricLine, ProviderError, Result, Usage, DAY_MS, WEEK_MS};

/// The Kimi CLI's public OAuth client.
pub const CLIENT_ID: &str = "17e5f671-d194-4dfb-9706-5516cb48c098";
pub const DEVICE_CODE_URL: &str = "https://auth.kimi.com/api/oauth/device_authorization";
pub const TOKEN_URL: &str = "https://auth.kimi.com/api/oauth/token";
pub const DEVICE_ID_KEY: &str = "device_id";
const USAGE_URL: &str = "https://api.kimi.com/coding/v1/usages";

/// Kimi's sign-in service identifies the device behind each request.
pub fn auth_request(http: &Client, url: &str, device_id: &str) -> reqwest::RequestBuilder {
    http.post(url)
        .header("Accept", "application/json")
        .header("X-Msh-Platform", "kimi_cli")
        .header("X-Msh-Version", env!("CARGO_PKG_VERSION"))
        .header("X-Msh-Device-Name", "AI Balance")
        .header("X-Msh-Device-Model", std::env::consts::OS)
        .header("X-Msh-Device-Id", device_id)
}

pub async fn refresh(http: &Client, refresh_token: &str, device_id: &str) -> Result<Grant> {
    let form = [("client_id", CLIENT_ID), ("grant_type", "refresh_token"), ("refresh_token", refresh_token)];
    let response = http::send(auth_request(http, TOKEN_URL, device_id).form(&form)).await?;
    // Kimi answers a dead refresh token with a bare 401 or 403.
    if matches!(response.status, 401 | 403) {
        return Err(ProviderError::SignInExpired);
    }
    Grant::from_renewal(&response)
}

pub async fn usage(http: &Client, credential: &Credential) -> Result<Usage> {
    let response = http::send(http.get(USAGE_URL).bearer_auth(credential.secret()).header("Accept", "application/json")).await?;
    Ok(Usage { plan: None, lines: map_usage(&response)? })
}

pub fn map_usage(response: &Resp) -> Result<Vec<MetricLine>> {
    if response.status == 404 {
        return Err(ProviderError::Unavailable("This Kimi account has no Kimi Code plan.".into()));
    }
    let body = response.require_json(false)?;
    let mut lines = Vec::new();
    // The plan's overall quota comes first; Kimi counts it by the week.
    lines.extend(quota_line(&body["usage"], "Weekly", Some(WEEK_MS)));
    for (index, item) in body["limits"].as_array().into_iter().flatten().enumerate() {
        let detail = if item["detail"].is_object() { &item["detail"] } else { item };
        let period = window_ms(&item["window"]);
        let label =
            ["name", "title"].iter().find_map(|key| http::text(&item[*key]).or_else(|| http::text(&detail[*key]))).unwrap_or_else(|| {
                match period {
                    Some(period) if period < DAY_MS => "Session".to_string(),
                    Some(period) if period <= WEEK_MS => "Weekly".to_string(),
                    _ => format!("Limit {}", index + 1),
                }
            });
        if lines.iter().any(|line: &MetricLine| line.label() == label) {
            continue;
        }
        lines.extend(quota_line(detail, &label, period));
    }
    if lines.is_empty() {
        return Err(ProviderError::Unavailable("Kimi reports no usage for this account.".into()));
    }
    Ok(lines)
}

/// A quota as a share of its limit. Kimi reports `used`, or only what `remaining`.
fn quota_line(data: &Value, label: &str, period: Option<i64>) -> Option<MetricLine> {
    let limit = http::num(&data["limit"]).filter(|limit| *limit > 0.0)?;
    let used = http::num(&data["used"]).or_else(|| http::num(&data["remaining"]).map(|remaining| limit - remaining))?;
    let resets_at = ["resetTime", "reset_time", "resetAt", "reset_at"].iter().find_map(|key| http::date_ms(&data[*key]));
    Some(MetricLine::percent(label, used / limit * 100.0, resets_at, period))
}

fn window_ms(window: &Value) -> Option<i64> {
    let duration = http::num(&window["duration"]).filter(|duration| *duration > 0.0)?;
    let unit = window["timeUnit"].as_str().unwrap_or_default();
    let unit_ms = if unit.contains("MINUTE") {
        60_000.0
    } else if unit.contains("HOUR") {
        3_600_000.0
    } else if unit.contains("DAY") {
        DAY_MS as f64
    } else {
        1000.0
    };
    Some((duration * unit_ms) as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SESSION_MS;
    use crate::providers::resp;
    use serde_json::json;

    #[test]
    fn maps_the_weekly_quota_and_the_session_window() {
        let lines = map_usage(&resp(
            200,
            json!({
                "usage": {"limit": "100", "used": "40", "remaining": "60", "resetTime": "2026-01-02T03:04:05.443553353Z"},
                "limits": [{
                    "window": {"duration": 300, "timeUnit": "TIME_UNIT_MINUTE"},
                    "detail": {"limit": "200", "remaining": "150", "resetTime": "2026-01-02T03:04:05Z"}
                }]
            }),
        ))
        .unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].label(), "Weekly");
        assert_eq!(lines[0].used_percent(), Some(40.0));
        assert_eq!(lines[1], MetricLine::percent("Session", 25.0, Some(1_767_323_045_000), Some(SESSION_MS)));
    }

    #[test]
    fn an_account_without_a_plan_or_usage_says_so() {
        assert!(matches!(map_usage(&resp(404, json!({}))), Err(ProviderError::Unavailable(_))));
        assert!(matches!(map_usage(&resp(200, json!({"usage": {}, "limits": []}))), Err(ProviderError::Unavailable(_))));
        assert!(matches!(map_usage(&resp(401, json!({}))), Err(ProviderError::SignInExpired)));
    }
}
