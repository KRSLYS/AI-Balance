//! Claude API: what an organization's API usage cost, from Anthropic's Admin API.
//!
//! This is the published cost report (`GET /v1/organizations/cost_report`), read with an Admin API
//! key the organization creates for this purpose. It covers Claude Console organizations; a
//! claude.ai plan's limits are a different thing and are read by `claude.rs`.

use chrono::{Datelike, TimeZone, Utc};
use reqwest::Client;
use serde_json::Value;

use crate::http::{self, Resp};
use crate::model::{now_ms, Format, MetricLine, ProviderError, Result, Usage, DAY_MS};

const COST_REPORT_URL: &str = "https://api.anthropic.com/v1/organizations/cost_report";
const API_VERSION: &str = "2023-06-01";
/// The report returns at most this many daily buckets, which covers any month.
const MAX_BUCKETS: &str = "31";

pub async fn usage(http: &Client, key: &str) -> Result<Usage> {
    let now = now_ms();
    let today = Utc.timestamp_millis_opt(now).single().ok_or(ProviderError::InvalidResponse)?;
    let month_start = Utc.with_ymd_and_hms(today.year(), today.month(), 1, 0, 0, 0).single().ok_or(ProviderError::InvalidResponse)?;
    let request = http
        .get(COST_REPORT_URL)
        .query(&[("starting_at", month_start.to_rfc3339_opts(chrono::SecondsFormat::Secs, true).as_str()), ("limit", MAX_BUCKETS)])
        .header("x-api-key", key)
        .header("anthropic-version", API_VERSION)
        .header("Accept", "application/json");
    Ok(Usage { plan: None, lines: map_report(&http::send(request).await?, now)? })
}

/// Adds up the daily buckets. Amounts arrive as decimal strings in cents.
pub fn map_report(response: &Resp, now: i64) -> Result<Vec<MetricLine>> {
    if matches!(response.status, 401 | 403) {
        return Err(ProviderError::KeyRejected("Anthropic rejected this key. The cost report needs an Admin API key.".into()));
    }
    let body = response.require_json(true)?;
    let buckets = body["data"].as_array().ok_or(ProviderError::InvalidResponse)?;
    let today_start = now - now.rem_euclid(DAY_MS);
    let (mut today, mut yesterday, mut month) = (0.0, 0.0, 0.0);
    for bucket in buckets {
        let start = http::date_ms(&bucket["starting_at"]).ok_or(ProviderError::InvalidResponse)?;
        let cents: f64 = bucket["results"].as_array().into_iter().flatten().filter_map(|result: &Value| http::num(&result["amount"])).sum();
        month += cents;
        if start == today_start {
            today += cents;
        } else if start == today_start - DAY_MS {
            yesterday += cents;
        }
    }
    let dollars = |label: &str, cents: f64| MetricLine::value(label, (cents / 100.0).max(0.0), Format::dollars());
    Ok(vec![dollars("Today", today), dollars("Yesterday", yesterday), dollars("This Month", month)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::resp;
    use serde_json::json;

    /// 2026-01-03T10:00:00Z
    const NOW: i64 = 1_767_434_400_000;

    #[test]
    fn adds_up_today_yesterday_and_the_month() {
        let lines = map_report(
            &resp(
                200,
                json!({"data": [
                    {"starting_at": "2026-01-01T00:00:00Z", "ending_at": "2026-01-02T00:00:00Z", "results": [{"amount": "1000", "currency": "USD"}]},
                    {"starting_at": "2026-01-02T00:00:00Z", "ending_at": "2026-01-03T00:00:00Z", "results": [{"amount": "250.5", "currency": "USD"}, {"amount": "49.5", "currency": "USD"}]},
                    {"starting_at": "2026-01-03T00:00:00Z", "ending_at": "2026-01-04T00:00:00Z", "results": [{"amount": "123.45", "currency": "USD"}]},
                    {"starting_at": "2026-01-04T00:00:00Z", "ending_at": "2026-01-05T00:00:00Z", "results": []}
                ], "has_more": false, "next_page": null}),
            ),
            NOW,
        )
        .unwrap();
        assert_eq!(lines[0], MetricLine::value("Today", 1.2345, Format::dollars()));
        assert_eq!(lines[1], MetricLine::value("Yesterday", 3.0, Format::dollars()));
        assert_eq!(lines[2], MetricLine::value("This Month", 14.2345, Format::dollars()));
    }

    #[test]
    fn a_quiet_month_reads_zero() {
        let lines = map_report(&resp(200, json!({"data": [], "has_more": false})), NOW).unwrap();
        assert!(lines.iter().all(|line| matches!(line, MetricLine::Value { value, .. } if *value == 0.0)));
    }

    #[test]
    fn a_key_that_is_not_an_admin_key_says_so() {
        assert!(matches!(map_report(&resp(401, json!({})), NOW), Err(ProviderError::KeyRejected(_))));
        assert!(matches!(map_report(&resp(200, json!({})), NOW), Err(ProviderError::InvalidResponse)));
    }
}
