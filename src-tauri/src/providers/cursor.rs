//! Cursor: the billing-cycle usage Cursor's dashboard reports for a cursor.com sign-in.

use reqwest::Client;
use serde_json::{json, Value};

use super::Grant;
use crate::http::{self, Resp};
use crate::model::{Credential, Format, MetricLine, ProviderError, Result, Usage};

pub const LOGIN_URL: &str = "https://cursor.com/loginDeepControl";
pub const POLL_URL: &str = "https://api2.cursor.sh/auth/poll";
const CLIENT_ID: &str = "KbZUR41cY7W6zRSdpSUJ7I7mLYBKOCmB";
const REFRESH_URL: &str = "https://api2.cursor.sh/oauth/token";
const SUMMARY_URL: &str = "https://cursor.com/api/usage-summary";
const PROFILE_URL: &str = "https://cursor.com/api/auth/me";

/// Cursor's dashboard takes the access token as a session cookie, prefixed with the user id from the token.
fn session_cookie(access_token: &str) -> Option<String> {
    let payload = http::jwt_payload(access_token)?;
    let subject = payload["sub"].as_str()?.trim();
    let user_id = subject.split('|').nth(1).unwrap_or(subject);
    (!user_id.is_empty()).then(|| format!("WorkosCursorSessionToken={user_id}%3A%3A{access_token}"))
}

async fn session_get(http: &Client, url: &str, access_token: &str) -> Result<Resp> {
    let cookie = session_cookie(access_token).ok_or(ProviderError::SignInExpired)?;
    http::send(http.get(url).header("Cookie", cookie).header("Accept", "application/json")).await
}

/// The account's email, or its name when Cursor reports no email.
pub async fn profile_name(http: &Client, access_token: &str) -> Option<String> {
    let body = session_get(http, PROFILE_URL, access_token).await.ok()?.require_json(false).ok()?;
    http::text(&body["email"]).or_else(|| http::text(&body["name"]))
}

/// Cursor answers with a new access token and no refresh token; the new token is the next refresh token.
pub async fn refresh(http: &Client, refresh_token: &str) -> Result<Grant> {
    let body = json!({"grant_type": "refresh_token", "client_id": CLIENT_ID, "refresh_token": refresh_token});
    let response = http::send(http.post(REFRESH_URL).json(&body)).await?;
    if response.json().map(|body| body["shouldLogout"].as_bool() == Some(true)).unwrap_or(false) {
        return Err(ProviderError::SignInExpired);
    }
    let mut grant = Grant::from_renewal(&response)?;
    grant.expires_at = http::jwt_expiry_ms(&grant.access_token);
    grant.refresh_token = grant.refresh_token.or_else(|| Some(grant.access_token.clone()));
    Ok(grant)
}

pub async fn usage(http: &Client, credential: &Credential) -> Result<Usage> {
    map_summary(&session_get(http, SUMMARY_URL, credential.secret()).await?)
}

pub fn map_summary(response: &Resp) -> Result<Usage> {
    let summary = response.require_json(false)?;
    let start = http::date_ms(&summary["billingCycleStart"]);
    let resets_at = http::date_ms(&summary["billingCycleEnd"]);
    let period = match (start, resets_at) {
        (Some(start), Some(end)) if end > start => Some(end - start),
        _ => None,
    };
    let individual = &summary["individualUsage"];
    let team = &summary["teamUsage"];
    let plan = &individual["plan"];
    let mut lines = Vec::new();

    let team_limited = summary["limitType"].as_str().map(str::to_lowercase).as_deref() == Some("team");
    let total = if team_limited { dollar_meter(&team["pooled"]) } else { None };
    if let Some((used, limit)) = total {
        lines.push(MetricLine::progress("Total Usage", used, limit, Format::dollars(), resets_at, period));
    } else if let Some(percent) = http::num(&plan["totalPercentUsed"]) {
        lines.push(MetricLine::percent("Total Usage", percent, resets_at, period));
    } else if let Some((used, limit)) = dollar_meter(&individual["overall"]).or_else(|| dollar_meter(&team["pooled"])) {
        lines.push(MetricLine::progress("Total Usage", used, limit, Format::dollars(), resets_at, period));
    }
    for (key, label) in [("autoPercentUsed", "Cursor Models"), ("apiPercentUsed", "Other Models")] {
        if let Some(percent) = http::num(&plan[key]) {
            lines.push(MetricLine::percent(label, percent, resets_at, period));
        }
    }
    // An Enterprise answer can hold a disabled individual bucket beside a live team one.
    for bucket in [&individual["onDemand"], &team["onDemand"]] {
        if bucket["enabled"].as_bool() == Some(false) {
            continue;
        }
        if let Some((used, limit)) = dollar_meter(bucket) {
            lines.push(MetricLine::progress("On-Demand", used, limit, Format::dollars(), resets_at, period));
            break;
        }
        if let Some(used_cents) = http::num(&bucket["used"]).filter(|used| *used > 0.0) {
            lines.push(MetricLine::value("On-Demand", used_cents / 100.0, Format::dollars()));
            break;
        }
    }
    if lines.is_empty() {
        return Err(ProviderError::Unavailable("Cursor reports no usage for this account.".into()));
    }
    Ok(Usage { plan: http::text(&summary["membershipType"]).map(|raw| http::title_case(&raw)), lines })
}

/// A bucket's used and limit in dollars. Cursor reports cents.
fn dollar_meter(bucket: &Value) -> Option<(f64, f64)> {
    if bucket["enabled"].as_bool() == Some(false) {
        return None;
    }
    let limit = http::num(&bucket["limit"]).filter(|limit| *limit > 0.0)?;
    let inferred = (limit - http::num(&bucket["remaining"]).unwrap_or(limit)).max(0.0);
    let used = http::num(&bucket["used"]).filter(|used| *used > 0.0).unwrap_or(inferred);
    Some((used / 100.0, limit / 100.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::resp;

    #[test]
    fn maps_plan_percentages_and_on_demand() {
        let usage = map_summary(&resp(
            200,
            json!({
                "membershipType": "pro",
                "billingCycleStart": "2026-01-01T00:00:00Z",
                "billingCycleEnd": "2026-01-31T00:00:00Z",
                "individualUsage": {
                    "plan": {"totalPercentUsed": 40.5, "autoPercentUsed": 10, "apiPercentUsed": 30.5},
                    "onDemand": {"enabled": true, "used": 250, "limit": 5000}
                }
            }),
        ))
        .unwrap();
        assert_eq!(usage.plan.as_deref(), Some("Pro"));
        let labels: Vec<_> = usage.lines.iter().map(MetricLine::label).collect();
        assert_eq!(labels, ["Total Usage", "Cursor Models", "Other Models", "On-Demand"]);
        assert_eq!(usage.lines[0].used_percent(), Some(40.5));
        assert_eq!(
            usage.lines[3],
            MetricLine::progress("On-Demand", 2.5, 50.0, Format::dollars(), Some(1_769_817_600_000), Some(30 * 86_400_000))
        );
    }

    #[test]
    fn a_disabled_individual_bucket_falls_back_to_the_team() {
        let usage = map_summary(&resp(
            200,
            json!({
                "limitType": "team",
                "individualUsage": {"onDemand": {"enabled": false, "limit": 0}},
                "teamUsage": {"pooled": {"limit": 10000, "remaining": 7500}, "onDemand": {"used": 120, "limit": 2000}}
            }),
        ))
        .unwrap();
        assert_eq!(usage.lines[0], MetricLine::progress("Total Usage", 25.0, 100.0, Format::dollars(), None, None));
        assert_eq!(usage.lines[1], MetricLine::progress("On-Demand", 1.2, 20.0, Format::dollars(), None, None));
    }

    #[test]
    fn the_session_cookie_uses_the_id_after_the_pipe() {
        // {"sub":"auth0|user_1","exp":1767323045}
        let token = "eyJhbGciOiJub25lIn0.eyJzdWIiOiJhdXRoMHx1c2VyXzEiLCJleHAiOjE3NjczMjMwNDV9.";
        assert_eq!(session_cookie(token).unwrap(), format!("WorkosCursorSessionToken=user_1%3A%3A{token}"));
    }
}
