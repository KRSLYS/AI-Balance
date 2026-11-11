//! OpenRouter: balance and spend for an API key or a Management key.

use reqwest::Client;
use serde_json::Value;

use crate::http::{self, Resp};
use crate::model::{Format, MetricLine, ProviderError, Result, Usage};

const KEY_URL: &str = "https://openrouter.ai/api/v1/key";
const KEYS_URL: &str = "https://openrouter.ai/api/v1/keys";
const CREDITS_URL: &str = "https://openrouter.ai/api/v1/credits";
const KEYS_PAGE_SIZE: usize = 100;
/// Bounds the page loop should OpenRouter ever ignore `offset`.
const MAX_KEYS_PAGES: usize = 20;
const PERIODS: [(&str, &str); 3] = [("Today", "usage_daily"), ("This Week", "usage_weekly"), ("This Month", "usage_monthly")];

async fn get(http: &Client, url: &str, key: &str) -> Result<Resp> {
    http::send(http.get(url).bearer_auth(key).header("Accept", "application/json")).await
}

pub async fn usage(http: &Client, key: &str) -> Result<Usage> {
    let key_body = get(http, KEY_URL, key).await?.require_json(true)?;
    let data = &key_body["data"];
    if !data.is_object() {
        return Err(ProviderError::InvalidResponse);
    }
    let management = data["is_management_key"].as_bool().or_else(|| data["is_provisioning_key"].as_bool()) == Some(true);

    let mut lines = Vec::new();
    // A Management key scoped to one workspace is refused the account's credits with a 403.
    let credits = get(http, CREDITS_URL, key).await?;
    if credits.is_ok() {
        lines.extend(credits.json().and_then(|body| balance_line(&body)));
    }
    if management {
        // A Management key spends nothing itself: the account's spend is the sum over its keys.
        let mut spend = [0.0; 3];
        for page in 0..MAX_KEYS_PAGES {
            let url = format!("{KEYS_URL}?include_disabled=true&offset={}", page * KEYS_PAGE_SIZE);
            let body = get(http, &url, key).await?.require_json(true)?;
            let keys = body["data"].as_array().ok_or(ProviderError::InvalidResponse)?;
            for listed in keys {
                add_spend(&mut spend, listed);
            }
            if keys.len() < KEYS_PAGE_SIZE {
                break;
            }
        }
        lines.extend(spend_lines(spend));
    } else {
        let mut spend = [0.0; 3];
        add_spend(&mut spend, data);
        lines.extend(spend_lines(spend));
        lines.extend(key_limit_line(data));
    }
    let plan = if management {
        Some("Management key".to_string())
    } else {
        data["is_free_tier"].as_bool().map(|free| if free { "Free" } else { "Pay-as-you-go" }.to_string())
    };
    Ok(Usage { plan, lines })
}

fn add_spend(total: &mut [f64; 3], key: &Value) {
    for (slot, (_, field)) in total.iter_mut().zip(PERIODS) {
        *slot += http::num(&key[field]).unwrap_or(0.0).max(0.0);
    }
}

fn spend_lines(spend: [f64; 3]) -> Vec<MetricLine> {
    PERIODS.iter().zip(spend).map(|((label, _), amount)| MetricLine::value(label, amount, Format::dollars())).collect()
}

/// Credits bought minus credits spent.
pub fn balance_line(body: &Value) -> Option<MetricLine> {
    let data = &body["data"];
    let bought = http::num(&data["total_credits"])?.max(0.0);
    let spent = http::num(&data["total_usage"])?.max(0.0);
    Some(MetricLine::value("Balance", (bought - spent).max(0.0), Format::dollars()))
}

pub fn key_limit_line(data: &Value) -> Option<MetricLine> {
    let limit = http::num(&data["limit"]).filter(|limit| *limit > 0.0)?;
    let remaining = http::num(&data["limit_remaining"])?.max(0.0);
    Some(MetricLine::progress("Key Limit", (limit - remaining).max(0.0), limit, Format::dollars(), None, None))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn balance_is_bought_minus_spent_and_never_negative() {
        let line = balance_line(&json!({"data": {"total_credits": 25, "total_usage": 10.5}})).unwrap();
        assert_eq!(line, MetricLine::value("Balance", 14.5, Format::dollars()));
        let overdrawn = balance_line(&json!({"data": {"total_credits": 1, "total_usage": 3}})).unwrap();
        assert_eq!(overdrawn, MetricLine::value("Balance", 0.0, Format::dollars()));
        assert_eq!(balance_line(&json!({"data": {}})), None);
    }

    #[test]
    fn key_limit_shows_only_when_the_key_has_one() {
        assert_eq!(key_limit_line(&json!({"limit": null, "limit_remaining": null})), None);
        let line = key_limit_line(&json!({"limit": 20, "limit_remaining": 5})).unwrap();
        assert_eq!(line.used_percent(), Some(75.0));
    }

    #[test]
    fn spend_adds_up_across_keys() {
        let mut spend = [0.0; 3];
        add_spend(&mut spend, &json!({"usage_daily": 1, "usage_weekly": 2, "usage_monthly": 3}));
        add_spend(&mut spend, &json!({"usage_daily": 0.5, "usage_weekly": null, "usage_monthly": -4}));
        assert_eq!(spend, [1.5, 2.0, 3.0]);
        assert_eq!(spend_lines(spend)[2], MetricLine::value("This Month", 3.0, Format::dollars()));
    }
}
