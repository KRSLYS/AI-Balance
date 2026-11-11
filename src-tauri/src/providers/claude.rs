//! Claude: the plan limits Anthropic reports for a claude.ai sign-in.

use reqwest::Client;
use serde_json::{json, Value};

use super::Grant;
use crate::http::{self, Resp};
use crate::model::{now_ms, Credential, Format, MetricLine, Result, Usage, SESSION_MS, WEEK_MS};

/// Claude Code's public OAuth client. Anthropic registers no client for third-party apps.
pub const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
pub const AUTHORIZE_URL: &str = "https://claude.ai/oauth/authorize";
pub const TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";
/// Where Anthropic shows the code to paste when the browser cannot return to the app.
pub const MANUAL_REDIRECT_URI: &str = "https://platform.claude.com/oauth/code/callback";
/// The least the usage read needs. The token can read the profile and usage and nothing else.
pub const SCOPES: &str = "user:profile";
const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const PROFILE_URL: &str = "https://api.anthropic.com/api/oauth/profile";
const PLAN_KEY: &str = "plan";
const PLAN_READ_AT_KEY: &str = "plan_read_at";
const PLAN_MAX_AGE_MS: i64 = 6 * 60 * 60 * 1000;

fn get(http: &Client, url: &str, token: &str) -> reqwest::RequestBuilder {
    http.get(url).bearer_auth(token.trim()).header("Accept", "application/json").header("anthropic-beta", "oauth-2025-04-20")
}

pub async fn profile(http: &Client, token: &str) -> Result<Value> {
    http::send(get(http, PROFILE_URL, token)).await?.require_json(false)
}

pub async fn refresh(http: &Client, refresh_token: &str) -> Result<Grant> {
    let body = json!({
        "grant_type": "refresh_token",
        "refresh_token": refresh_token,
        "client_id": CLIENT_ID,
        "scope": SCOPES,
    });
    Grant::from_renewal(&http::send(http.post(TOKEN_URL).json(&body)).await?)
}

pub async fn usage(http: &Client, credential: &mut Credential) -> Result<Usage> {
    let response = http::send(get(http, USAGE_URL, credential.secret())).await?;
    let lines = map_usage(&response)?;
    refresh_plan(http, credential).await;
    Ok(Usage { plan: credential.extra(PLAN_KEY).map(str::to_string), lines })
}

/// The plan changes rarely and Anthropic budgets these endpoints tightly, so it is read every few hours.
async fn refresh_plan(http: &Client, credential: &mut Credential) {
    let read_at = credential.extra(PLAN_READ_AT_KEY).and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
    if now_ms() - read_at < PLAN_MAX_AGE_MS {
        return;
    }
    let plan = profile(http, credential.secret()).await.ok().and_then(|body| plan_name(&body));
    if let Credential::OAuth { extras, .. } = credential {
        extras.insert(PLAN_READ_AT_KEY.into(), now_ms().to_string());
        if let Some(plan) = plan {
            extras.insert(PLAN_KEY.into(), plan);
        }
    }
}

pub fn map_usage(response: &Resp) -> Result<Vec<MetricLine>> {
    let body = response.require_json(false)?;
    let mut lines = Vec::new();
    for (key, label, period) in [
        ("five_hour", "Session", SESSION_MS),
        ("seven_day", "Weekly", WEEK_MS),
        ("seven_day_sonnet", "Sonnet", WEEK_MS),
        ("seven_day_opus", "Opus", WEEK_MS),
    ] {
        if let Some(used) = http::num(&body[key]["utilization"]) {
            lines.push(MetricLine::percent(label, used, http::date_ms(&body[key]["resets_at"]), Some(period)));
        }
    }
    // Model-scoped weekly limits arrive in `limits`, named by the model they cover.
    for entry in body["limits"].as_array().into_iter().flatten() {
        let name = http::text(&entry["scope"]["model"]["display_name"]);
        if let (Some("weekly_scoped"), Some(name), Some(used)) = (entry["kind"].as_str(), name, http::num(&entry["percent"])) {
            if !lines.iter().any(|line| line.label() == name) {
                lines.push(MetricLine::percent(&name, used, http::date_ms(&entry["resets_at"]), Some(WEEK_MS)));
            }
        }
    }
    let extra = &body["extra_usage"];
    if extra["is_enabled"].as_bool() == Some(true) {
        let code = http::text(&extra["currency"]).unwrap_or_else(|| "USD".into());
        let minor_units = 10f64.powf(http::num(&extra["decimal_places"]).unwrap_or(2.0));
        let used = http::num(&extra["used_credits"]).unwrap_or(0.0).round() / minor_units;
        let format = Format::Currency { code };
        match http::num(&extra["monthly_limit"]).filter(|limit| *limit > 0.0) {
            Some(limit) => lines.push(MetricLine::progress("Extra Usage", used, limit.round() / minor_units, format, None, None)),
            None => lines.push(MetricLine::value("Extra Usage", used, format)),
        }
    }
    Ok(lines)
}

/// `claude_max` with a `…_20x` tier reads `Max 20x`.
pub fn plan_name(profile: &Value) -> Option<String> {
    let organization = &profile["organization"];
    let kind = http::text(&organization["organization_type"])?;
    let base = http::title_case(kind.strip_prefix("claude_").unwrap_or(&kind));
    if base.is_empty() {
        return None;
    }
    let tier = http::text(&organization["rate_limit_tier"]).unwrap_or_default();
    let multiplier =
        tier.split('_').find(|part| part.len() > 1 && part.ends_with('x') && part[..part.len() - 1].chars().all(|c| c.is_ascii_digit()));
    Some(match multiplier {
        Some(multiplier) => format!("{base} {multiplier}"),
        None => base,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ProviderError;
    use crate::providers::resp;

    #[test]
    fn maps_windows_scoped_limits_and_extra_usage() {
        let lines = map_usage(&resp(
            200,
            json!({
                "five_hour": {"utilization": 42.0, "resets_at": "2026-01-02T03:04:05Z"},
                "seven_day": {"utilization": 130, "resets_at": null},
                "seven_day_sonnet": null,
                "limits": [
                    {"kind": "weekly_scoped", "percent": 12, "scope": {"model": {"display_name": "Fable"}}},
                    {"kind": "other", "percent": 99, "scope": {"model": {"display_name": "Ignored"}}}
                ],
                "extra_usage": {"is_enabled": true, "used_credits": 1234, "monthly_limit": 5000, "currency": "EUR"}
            }),
        ))
        .unwrap();
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0], MetricLine::percent("Session", 42.0, Some(1_767_323_045_000), Some(SESSION_MS)));
        assert_eq!(lines[1].used_percent(), Some(100.0));
        assert_eq!(lines[2].label(), "Fable");
        assert_eq!(lines[3], MetricLine::progress("Extra Usage", 12.34, 50.0, Format::Currency { code: "EUR".into() }, None, None));
    }

    #[test]
    fn extra_usage_hides_while_off() {
        let lines = map_usage(&resp(200, json!({"extra_usage": {"is_enabled": false, "used_credits": 5}}))).unwrap();
        assert!(lines.is_empty());
    }

    #[test]
    fn auth_and_rate_limit_failures_are_told_apart() {
        assert!(matches!(map_usage(&resp(401, json!({}))), Err(ProviderError::SignInExpired)));
        assert!(matches!(map_usage(&resp(429, json!({}))), Err(ProviderError::RateLimited { .. })));
    }

    #[test]
    fn plan_names_include_the_tier_multiplier() {
        let plan = |kind: &str, tier: &str| plan_name(&json!({"organization": {"organization_type": kind, "rate_limit_tier": tier}}));
        assert_eq!(plan("claude_max", "default_claude_max_20x").as_deref(), Some("Max 20x"));
        assert_eq!(plan("claude_pro", "default_claude_ai").as_deref(), Some("Pro"));
        assert_eq!(plan_name(&json!({"organization": {}})), None);
    }
}
