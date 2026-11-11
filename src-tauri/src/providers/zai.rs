//! Z.ai: the GLM Coding Plan quotas for an API key.

use reqwest::Client;
use serde_json::Value;

use crate::http::{self, Resp};
use crate::model::{Format, MetricLine, ProviderError, Result, Usage, DAY_MS, MONTH_MS};

const SUBSCRIPTION_URL: &str = "https://api.z.ai/api/biz/subscription/list";
const QUOTA_URL: &str = "https://api.z.ai/api/monitor/usage/quota/limit";

async fn get(http: &Client, url: &str, key: &str) -> Result<Resp> {
    http::send(http.get(url).bearer_auth(key).header("Accept", "application/json")).await
}

pub async fn usage(http: &Client, key: &str) -> Result<Usage> {
    let lines = map_quota(&get(http, QUOTA_URL, key).await?)?;
    // The plan name is a nicety: a failed read leaves the card without one.
    let plan = match get(http, SUBSCRIPTION_URL, key).await {
        Ok(response) if response.is_ok() => response.json().and_then(|body| http::text(&body["data"][0]["productName"])),
        _ => None,
    };
    Ok(Usage { plan, lines })
}

pub fn map_quota(response: &Resp) -> Result<Vec<MetricLine>> {
    let root = response.require_json(true)?;
    if root["success"].as_bool() == Some(false) {
        if root["msg"].as_str().map(|msg| msg.to_lowercase().contains("coding plan")).unwrap_or(false) {
            return Err(ProviderError::Unavailable("No active GLM Coding Plan on this key.".into()));
        }
        if http::num(&root["code"]) == Some(401.0) {
            return Err(ProviderError::KeyRejected("Z.ai rejected this API key.".into()));
        }
    }
    let container = if root["data"].is_object() { &root["data"] } else { &root };
    let limits = container["limits"].as_array().ok_or(ProviderError::InvalidResponse)?;

    let mut lines = Vec::new();
    for entry in limits {
        // Z.ai has named the kind in `type` and in `name` across revisions.
        let kind = entry["type"].as_str().or_else(|| entry["name"].as_str()).unwrap_or_default();
        let resets_at = http::date_ms(&entry["nextResetTime"]);
        match kind {
            "TOKENS_LIMIT" | "CREDIT_LIMIT" => {
                let (Some(period), Some(used)) = (window_ms(entry), http::num(&entry["percentage"])) else { continue };
                let label = if period < DAY_MS { "Session" } else { "Weekly" };
                lines.push(MetricLine::percent(label, used, resets_at, Some(period)));
            }
            "TIME_LIMIT" => {
                let (Some(used), Some(limit)) = (http::num(&entry["currentValue"]), http::num(&entry["usage"])) else { continue };
                lines.push(MetricLine::progress(
                    "Web Searches",
                    used.max(0.0),
                    limit.max(0.0),
                    Format::count("searches"),
                    resets_at,
                    Some(MONTH_MS),
                ));
            }
            _ => {}
        }
    }
    Ok(lines)
}

/// A limit's window: `number` units of hours (3), days (4), months (5) or weeks (6).
fn window_ms(entry: &Value) -> Option<i64> {
    let unit_ms = match http::num(&entry["unit"])? as i64 {
        3 => 60 * 60 * 1000,
        4 => DAY_MS,
        5 => MONTH_MS,
        6 => 7 * DAY_MS,
        _ => return None,
    };
    let count = http::num(&entry["number"]).filter(|count| *count > 0.0)?;
    Some((unit_ms as f64 * count) as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SESSION_MS;
    use crate::providers::resp;
    use serde_json::json;

    #[test]
    fn maps_token_windows_and_web_searches() {
        let lines = map_quota(&resp(
            200,
            json!({"success": true, "data": {"limits": [
                {"type": "TOKENS_LIMIT", "unit": 3, "number": 5, "percentage": 37, "nextResetTime": 1767323045000_i64},
                {"type": "TOKENS_LIMIT", "unit": 6, "number": 1, "percentage": 80},
                {"type": "TIME_LIMIT", "currentValue": 12, "usage": 100},
                {"type": "TOKENS_LIMIT", "unit": 99, "number": 1, "percentage": 1}
            ]}}),
        ))
        .unwrap();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], MetricLine::percent("Session", 37.0, Some(1_767_323_045_000), Some(SESSION_MS)));
        assert_eq!(lines[1].label(), "Weekly");
        assert_eq!(lines[2].used_percent(), Some(12.0));
    }

    #[test]
    fn a_key_without_a_plan_or_a_rejected_key_says_so() {
        let no_plan = resp(200, json!({"success": false, "msg": "No Coding Plan found"}));
        assert!(matches!(map_quota(&no_plan), Err(ProviderError::Unavailable(_))));
        let rejected = resp(200, json!({"success": false, "code": 401, "msg": "token expired"}));
        assert!(matches!(map_quota(&rejected), Err(ProviderError::KeyRejected(_))));
        assert!(matches!(map_quota(&resp(401, json!({}))), Err(ProviderError::KeyRejected(_))));
    }
}
