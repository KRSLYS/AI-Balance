//! Devin: the daily and weekly quota Windsurf reports for a Windsurf sign-in.

use reqwest::Client;
use serde_json::{json, Value};

use crate::http::{self, Resp};
use crate::model::{Credential, Format, MetricLine, ProviderError, Result, Usage, DAY_MS, WEEK_MS};

/// The page that shows a one-time token to paste into the app.
pub const TOKEN_PAGE_URL: &str = "https://windsurf.com/profile";
pub const REGISTER_URL: &str = "https://api.codeium.com/register_user/";
pub const SERVER_KEY: &str = "api_server_url";
const DEFAULT_SERVER: &str = "https://server.codeium.com";
const STATUS_PATH: &str = "/exa.seat_management_pb.SeatManagementService/GetUserStatus";
const CLIENT_VERSION: &str = "1.108.2";

/// The API server an account was told to use, when it is one of Windsurf's own. The address comes
/// from a server answer and the account's key is sent to it, so anything else is refused.
pub fn trusted_server(address: &str) -> Option<String> {
    let url = url::Url::parse(address).ok()?;
    let host = url.host_str()?;
    let trusted = ["codeium.com", "windsurf.com"].iter().any(|domain| host == *domain || host.ends_with(&format!(".{domain}")));
    (url.scheme() == "https" && trusted).then(|| address.trim_end_matches('/').to_string())
}

pub async fn user_status(http: &Client, credential: &Credential) -> Result<Resp> {
    let server = credential.extra(SERVER_KEY).and_then(trusted_server).unwrap_or_else(|| DEFAULT_SERVER.to_string());
    let body = json!({"metadata": {
        "apiKey": credential.secret(),
        "ideName": "devin",
        "ideVersion": CLIENT_VERSION,
        "extensionName": "devin",
        "extensionVersion": CLIENT_VERSION,
        "locale": "en"
    }});
    http::send(http.post(format!("{server}{STATUS_PATH}")).header("Connect-Protocol-Version", "1").json(&body)).await
}

pub async fn usage(http: &Client, credential: &Credential) -> Result<Usage> {
    map_status(&user_status(http, credential).await?.require_json(false)?)
}

pub fn map_status(body: &Value) -> Result<Usage> {
    let status = &body["userStatus"];
    if !status.is_object() {
        return Err(ProviderError::InvalidResponse);
    }
    let plan = &status["planStatus"];
    let quota = |remaining: &str, reset: &str| -> Option<(f64, Option<i64>)> {
        let resets_at = http::num(&plan[reset]).map(|seconds| (seconds * 1000.0) as i64);
        match http::num(&plan[remaining]) {
            Some(remaining) => Some((100.0 - remaining, resets_at)),
            // A reset time without a percentage is a quota that is used up.
            None => resets_at.map(|at| (100.0, Some(at))),
        }
    };
    let daily = quota("dailyQuotaRemainingPercent", "dailyQuotaResetAtUnix");
    let weekly = quota("weeklyQuotaRemainingPercent", "weeklyQuotaResetAtUnix");
    let mut lines = Vec::new();
    if http::boolean(&plan["planInfo"]["hideDailyQuota"]) == Some(true) {
        // Such a plan shows one quota: the weekly one, or the daily one in its place.
        if let Some((used, resets_at)) = weekly.or(daily.map(|(used, _)| (used, None))) {
            lines.push(MetricLine::percent("Weekly", used, resets_at, Some(WEEK_MS)));
        }
    } else {
        lines.extend(daily.map(|(used, resets_at)| MetricLine::percent("Daily", used, resets_at, Some(DAY_MS))));
        lines.extend(weekly.map(|(used, resets_at)| MetricLine::percent("Weekly", used, resets_at, Some(WEEK_MS))));
    }
    if let Some(micros) = http::num(&plan["overageBalanceMicros"]) {
        lines.push(MetricLine::value("Extra Usage", micros.max(0.0) / 1_000_000.0, Format::dollars()));
    }
    if lines.is_empty() {
        return Err(ProviderError::Unavailable("Devin reports no quota for this account.".into()));
    }
    Ok(Usage { plan: http::text(&plan["planInfo"]["planName"]), lines })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_both_quotas_and_the_balance() {
        let usage = map_status(&json!({"userStatus": {"planStatus": {
            "planInfo": {"planName": "Pro"},
            "dailyQuotaRemainingPercent": 80, "dailyQuotaResetAtUnix": 1767323045,
            "weeklyQuotaRemainingPercent": 25,
            "overageBalanceMicros": "12500000"
        }}}))
        .unwrap();
        assert_eq!(usage.plan.as_deref(), Some("Pro"));
        assert_eq!(usage.lines[0], MetricLine::percent("Daily", 20.0, Some(1_767_323_045_000), Some(DAY_MS)));
        assert_eq!(usage.lines[1].used_percent(), Some(75.0));
        assert_eq!(usage.lines[2], MetricLine::value("Extra Usage", 12.5, Format::dollars()));
    }

    #[test]
    fn a_plan_that_hides_the_daily_quota_shows_one_weekly_row() {
        let usage =
            map_status(&json!({"userStatus": {"planStatus": {"planInfo": {"hideDailyQuota": true}, "dailyQuotaRemainingPercent": 60}}}))
                .unwrap();
        assert_eq!(usage.lines, [MetricLine::percent("Weekly", 40.0, None, Some(WEEK_MS))]);
    }

    #[test]
    fn an_empty_account_says_so() {
        assert!(matches!(map_status(&json!({"userStatus": {}})), Err(ProviderError::Unavailable(_))));
        assert!(matches!(map_status(&json!({})), Err(ProviderError::InvalidResponse)));
    }

    #[test]
    fn only_windsurf_servers_are_trusted_with_the_key() {
        assert_eq!(trusted_server("https://server.self-serve.windsurf.com/").as_deref(), Some("https://server.self-serve.windsurf.com"));
        assert_eq!(trusted_server("https://server.codeium.com").as_deref(), Some("https://server.codeium.com"));
        assert_eq!(trusted_server("http://server.codeium.com"), None);
        assert_eq!(trusted_server("https://codeium.com.evil.example"), None);
        assert_eq!(trusted_server("https://evilcodeium.com"), None);
        assert_eq!(trusted_server("not a url"), None);
    }
}
