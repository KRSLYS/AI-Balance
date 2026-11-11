//! Ollama: the cloud plan's windows and purchased credits for an API key.

use reqwest::Client;
use serde_json::Value;

use crate::http::{self, Resp};
use crate::model::{Format, MetricLine, ProviderError, Result, Usage, SESSION_MS, WEEK_MS};

const BALANCE_URL: &str = "https://ollama.com/api/balance";
const ACCOUNT_URL: &str = "https://ollama.com/api/me";

pub async fn usage(http: &Client, key: &str) -> Result<Usage> {
    let balance = http::send(http.get(BALANCE_URL).bearer_auth(key).header("Accept", "application/json")).await?;
    let lines = map_balance(&balance)?;
    let plan = account(http, key).await.and_then(|body| field(&body, "Plan")).map(|plan| http::title_case(&plan));
    Ok(Usage { plan, lines })
}

pub async fn account(http: &Client, key: &str) -> Option<Value> {
    let response = http::send(http.post(ACCOUNT_URL).bearer_auth(key).header("Accept", "application/json")).await.ok()?;
    response.is_ok().then(|| response.json()).flatten()
}

/// ollama.com capitalizes its JSON keys; a local Ollama server proxying the same answer lowercases them.
pub fn field(body: &Value, name: &str) -> Option<String> {
    http::text(&body[name]).or_else(|| http::text(&body[name.to_lowercase()]))
}

pub fn map_balance(response: &Resp) -> Result<Vec<MetricLine>> {
    let root = response.require_json(true)?;
    let included = &root["included"];
    if !included.is_object() {
        return Err(ProviderError::InvalidResponse);
    }
    let mut lines = Vec::new();
    for (key, label, period) in [("session", "Session", SESSION_MS), ("weekly", "Weekly", WEEK_MS)] {
        if let Some(remaining) = http::num(&included[key]["remaining_percent"]) {
            let resets_at = http::date_ms(&included[key]["resets_at"]);
            lines.push(MetricLine::percent(label, 100.0 - remaining, resets_at, resets_at.map(|_| period)));
        }
    }
    let allowance = http::num(&included["allowance_usd"]).filter(|allowance| *allowance > 0.0);
    if let (Some(allowance), Some(balance)) = (allowance, http::num(&included["balance_usd"])) {
        let from = http::date_ms(&included["period"]["from"]);
        let until = http::date_ms(&included["period"]["until"]);
        let period = match (from, until) {
            (Some(from), Some(until)) if until > from => Some(until - from),
            _ => None,
        };
        let used = (allowance - balance).clamp(0.0, allowance);
        lines.push(MetricLine::progress("Monthly", used, allowance, Format::dollars(), until, period));
    }
    if lines.is_empty() {
        return Err(ProviderError::Unavailable("Ollama reports no plan usage for this key.".into()));
    }
    if let Some(balance) = http::num(&root["purchased"]["balance_usd"]) {
        lines.push(MetricLine::value("Purchased Credits", balance.max(0.0), Format::dollars()));
    }
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::resp;
    use serde_json::json;

    #[test]
    fn maps_windows_allowance_and_purchased_credits() {
        let lines = map_balance(&resp(
            200,
            json!({
                "included": {
                    "session": {"remaining_percent": 70, "resets_at": "2026-01-02T03:04:05Z"},
                    "weekly": {"remaining_percent": 20},
                    "allowance_usd": 20, "balance_usd": 15,
                    "period": {"from": "2026-01-01T00:00:00Z", "until": "2026-01-31T00:00:00Z"}
                },
                "purchased": {"balance_usd": 4.25}
            }),
        ))
        .unwrap();
        assert_eq!(lines[0], MetricLine::percent("Session", 30.0, Some(1_767_323_045_000), Some(SESSION_MS)));
        assert_eq!(lines[1], MetricLine::percent("Weekly", 80.0, None, None));
        assert_eq!(lines[2].used_percent(), Some(25.0));
        assert_eq!(lines[3], MetricLine::value("Purchased Credits", 4.25, Format::dollars()));
    }

    #[test]
    fn account_fields_read_in_either_case() {
        assert_eq!(field(&json!({"Email": "a@b.c"}), "Email").as_deref(), Some("a@b.c"));
        assert_eq!(field(&json!({"plan": "pro"}), "Plan").as_deref(), Some("pro"));
    }

    #[test]
    fn a_rejected_key_is_reported_as_one() {
        assert!(matches!(map_balance(&resp(401, json!({}))), Err(ProviderError::KeyRejected(_))));
    }
}
