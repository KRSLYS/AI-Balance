//! GitHub Copilot: the monthly quotas GitHub reports for a GitHub sign-in.

use reqwest::Client;
use serde_json::Value;

use crate::http::{self, Resp};
use crate::model::{Credential, Format, MetricLine, ProviderError, Result, Usage, MONTH_MS};

/// The public OAuth client of GitHub's own Copilot editor plugins.
pub const CLIENT_ID: &str = "Iv1.b507a08c87ecfe98";
pub const DEVICE_CODE_URL: &str = "https://github.com/login/device/code";
pub const DEVICE_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
pub const SCOPES: &str = "read:user";
pub const USER_URL: &str = "https://api.github.com/user";
const USAGE_URL: &str = "https://api.github.com/copilot_internal/user";

pub async fn usage(http: &Client, credential: &Credential) -> Result<Usage> {
    // This endpoint takes the `token` scheme and expects an editor's headers.
    let request = http
        .get(USAGE_URL)
        .header("Authorization", format!("token {}", credential.secret()))
        .header("Accept", "application/json")
        .header("Editor-Version", "vscode/1.96.2")
        .header("Editor-Plugin-Version", "copilot-chat/0.26.7")
        .header("X-Github-Api-Version", "2025-04-01");
    map_usage(&http::send(request).await?)
}

pub fn map_usage(response: &Resp) -> Result<Usage> {
    let body = response.require_json(false)?;
    let plan = http::text(&body["copilot_plan"]).map(|raw| http::title_case(&raw));
    let resets_at = http::date_ms(&body["quota_reset_date"]).or_else(|| http::date_ms(&body["limited_user_reset_date"]));
    let snapshots = &body["quota_snapshots"];
    let premium = &snapshots["premium_interactions"];

    let mut lines = Vec::new();
    if let Some(line) = snapshot_line("Premium", premium, resets_at) {
        lines.push(line);
        if http::boolean(&premium["overage_permitted"]) == Some(true) {
            let overage = http::num(&premium["overage_count"]).unwrap_or(0.0).max(0.0);
            lines.push(MetricLine::value("Extra Usage", overage, Format::count("requests")));
        }
    }
    lines.extend(snapshot_line("Chat", &snapshots["chat"], resets_at));
    lines.extend(snapshot_line("Completions", &snapshots["completions"], resets_at));

    if lines.is_empty() {
        // The free plan reports what is left of fixed monthly quotas.
        for (label, key) in [("Chat", "chat"), ("Completions", "completions")] {
            let total = http::num(&body["monthly_quotas"][key]).filter(|total| *total > 0.0);
            if let (Some(total), Some(remaining)) = (total, http::num(&body["limited_user_quotas"][key])) {
                let used = (total - remaining).max(0.0) / total * 100.0;
                lines.push(MetricLine::percent(label, used, resets_at, Some(MONTH_MS)));
            }
        }
    }
    if lines.is_empty() {
        return Err(ProviderError::Unavailable("Copilot reports no usage for this account.".into()));
    }
    Ok(Usage { plan, lines })
}

/// `None` for an unlimited quota, which GitHub marks with a flag or with `-1`.
fn snapshot_line(label: &str, snapshot: &Value, resets_at: Option<i64>) -> Option<MetricLine> {
    if !snapshot.is_object() {
        return None;
    }
    let entitlement = http::num(&snapshot["entitlement"]);
    let remaining = http::num(&snapshot["remaining"]);
    if http::boolean(&snapshot["unlimited"]) == Some(true)
        || entitlement == Some(-1.0)
        || remaining == Some(-1.0)
        || entitlement == Some(0.0)
    {
        return None;
    }
    let used = match (http::num(&snapshot["percent_remaining"]), entitlement, remaining) {
        (Some(percent_remaining), _, _) => 100.0 - percent_remaining,
        (None, Some(entitlement), Some(remaining)) if entitlement > 0.0 => 100.0 - remaining / entitlement * 100.0,
        _ => return None,
    };
    Some(MetricLine::percent(label, used, resets_at, Some(MONTH_MS)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::resp;
    use serde_json::json;

    #[test]
    fn maps_paid_plan_snapshots_and_skips_unlimited() {
        let usage = map_usage(&resp(200, json!({
            "copilot_plan": "individual_pro",
            "quota_reset_date": "2026-01-02",
            "quota_snapshots": {
                "premium_interactions": {"entitlement": 300, "remaining": 75, "percent_remaining": 25.0, "overage_permitted": true, "overage_count": 4},
                "chat": {"unlimited": true, "entitlement": 0, "remaining": 0},
                "completions": {"entitlement": -1, "remaining": -1}
            }
        }))).unwrap();
        assert_eq!(usage.plan.as_deref(), Some("Individual Pro"));
        assert_eq!(usage.lines.len(), 2);
        assert_eq!(usage.lines[0], MetricLine::percent("Premium", 75.0, Some(1_767_312_000_000), Some(MONTH_MS)));
        assert_eq!(usage.lines[1], MetricLine::value("Extra Usage", 4.0, Format::count("requests")));
    }

    #[test]
    fn maps_free_plan_quotas() {
        let usage = map_usage(&resp(
            200,
            json!({
                "limited_user_quotas": {"chat": 10, "completions": 2000},
                "monthly_quotas": {"chat": 50, "completions": 2000}
            }),
        ))
        .unwrap();
        assert_eq!(usage.lines[0].used_percent(), Some(80.0));
        assert_eq!(usage.lines[1].used_percent(), Some(0.0));
    }

    #[test]
    fn an_account_without_quotas_says_so() {
        assert!(matches!(map_usage(&resp(200, json!({}))), Err(ProviderError::Unavailable(_))));
    }
}
