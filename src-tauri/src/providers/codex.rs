//! ChatGPT: the Codex limits OpenAI reports for a ChatGPT sign-in.

use reqwest::Client;
use serde_json::Value;

use super::Grant;
use crate::http::{self, Resp};
use crate::model::{now_ms, Credential, Format, MetricLine, Result, Usage, DAY_MS};

/// The Codex CLI's public OAuth client.
pub const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub const AUTHORIZE_URL: &str = "https://auth.openai.com/oauth/authorize";
pub const TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
pub const SCOPES: &str = "openid profile email offline_access";
pub const ACCOUNT_ID_KEY: &str = "chatgpt_account_id";
const USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";

pub async fn refresh(http: &Client, refresh_token: &str) -> Result<Grant> {
    let form = [("grant_type", "refresh_token"), ("client_id", CLIENT_ID), ("refresh_token", refresh_token)];
    let mut grant = Grant::from_renewal(&http::send(http.post(TOKEN_URL).form(&form)).await?)?;
    grant.expires_at = http::jwt_expiry_ms(&grant.access_token).or(grant.expires_at);
    Ok(grant)
}

pub async fn usage(http: &Client, credential: &Credential) -> Result<Usage> {
    let mut request = http.get(USAGE_URL).bearer_auth(credential.secret()).header("Accept", "application/json");
    if let Some(account_id) = credential.extra(ACCOUNT_ID_KEY) {
        request = request.header("ChatGPT-Account-Id", account_id);
    }
    map_usage(&http::send(request).await?, now_ms())
}

pub fn map_usage(response: &Resp, now: i64) -> Result<Usage> {
    let body = response.require_json(false)?;
    let mut lines = window_lines(&body["rate_limit"], "Session", "Weekly", now);
    for entry in body["additional_rate_limits"].as_array().into_iter().flatten() {
        let Some(name) = http::text(&entry["limit_name"]).or_else(|| http::text(&entry["metered_feature"])) else { continue };
        let name = http::title_case(&name);
        lines.extend(window_lines(&entry["rate_limit"], &name, &format!("{name} Weekly"), now));
    }
    let credits = &body["credits"];
    if http::boolean(&credits["has_credits"]) != Some(false) {
        if let Some(balance) = http::num(&credits["balance"]) {
            lines.push(MetricLine::value("Credits", balance.max(0.0).floor(), Format::count("credits")));
        }
    }
    let limit = &body["spend_control"]["individual_limit"];
    if let (Some(used), Some(cap)) = (http::num(&limit["used"]), http::num(&limit["limit"])) {
        if used >= 0.0 && cap > 0.0 {
            let resets_at = http::num(&limit["reset_at"]).map(|seconds| (seconds * 1000.0) as i64);
            lines.push(MetricLine::progress("Credit Limit", used, cap, Format::count("credits"), resets_at, None));
        }
    }
    Ok(Usage { plan: http::text(&body["plan_type"]).map(|raw| plan_name(&raw)), lines })
}

/// The short window usually arrives as `primary_window` and the weekly one as `secondary_window`, but
/// a lone weekly limit can sit in the primary slot. A window is named by its own length when it has one.
fn window_lines(rate_limit: &Value, short_label: &str, long_label: &str, now: i64) -> Vec<MetricLine> {
    let mut lines: Vec<MetricLine> = Vec::new();
    for (slot, fallback) in [("primary_window", short_label), ("secondary_window", long_label)] {
        let window = &rate_limit[slot];
        let Some(used) = http::num(&window["used_percent"]) else { continue };
        let period = http::num(&window["limit_window_seconds"]).map(|seconds| (seconds * 1000.0) as i64);
        let label = match period {
            Some(period) if period < DAY_MS => short_label,
            Some(_) => long_label,
            None => fallback,
        };
        if lines.iter().any(|line| line.label() == label) {
            continue;
        }
        let resets_at = http::num(&window["reset_at"])
            .map(|seconds| (seconds * 1000.0) as i64)
            .or_else(|| http::num(&window["reset_after_seconds"]).map(|seconds| now + (seconds * 1000.0) as i64));
        lines.push(MetricLine::percent(label, used, resets_at, period));
    }
    lines
}

fn plan_name(raw: &str) -> String {
    match raw.to_lowercase().as_str() {
        "prolite" => "Pro 100".into(),
        "pro" => "Pro 200".into(),
        "promax" => "Pro 500".into(),
        _ => http::title_case(raw),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{SESSION_MS, WEEK_MS};
    use crate::providers::resp;
    use serde_json::json;

    #[test]
    fn maps_both_windows_credits_and_plan() {
        let usage = map_usage(
            &resp(
                200,
                json!({
                    "plan_type": "plus",
                    "rate_limit": {
                        "primary_window": {"used_percent": 25, "limit_window_seconds": 18000, "reset_at": 1767323045},
                        "secondary_window": {"used_percent": 60, "limit_window_seconds": 604800, "reset_after_seconds": 100}
                    },
                    "credits": {"has_credits": true, "balance": "12.9"}
                }),
            ),
            1_000,
        )
        .unwrap();
        assert_eq!(usage.plan.as_deref(), Some("Plus"));
        assert_eq!(usage.lines[0], MetricLine::percent("Session", 25.0, Some(1_767_323_045_000), Some(SESSION_MS)));
        assert_eq!(usage.lines[1], MetricLine::percent("Weekly", 60.0, Some(101_000), Some(WEEK_MS)));
        assert_eq!(usage.lines[2], MetricLine::value("Credits", 12.0, Format::count("credits")));
    }

    #[test]
    fn a_lone_weekly_window_in_the_primary_slot_is_weekly() {
        let usage = map_usage(
            &resp(
                200,
                json!({
                    "rate_limit": {"primary_window": {"used_percent": 10, "limit_window_seconds": 604800}},
                    "credits": {"has_credits": false, "balance": 3}
                }),
            ),
            0,
        )
        .unwrap();
        assert_eq!(usage.lines.len(), 1);
        assert_eq!(usage.lines[0].label(), "Weekly");
    }

    #[test]
    fn additional_limits_take_their_own_name() {
        let usage = map_usage(
            &resp(
                200,
                json!({
                    "additional_rate_limits": [{"limit_name": "spark", "rate_limit": {"primary_window": {"used_percent": 5}}}]
                }),
            ),
            0,
        )
        .unwrap();
        assert_eq!(usage.lines[0].label(), "Spark");
    }
}
