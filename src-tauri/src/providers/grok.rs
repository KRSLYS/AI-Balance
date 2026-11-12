//! Grok: the weekly limit xAI reports for an xAI sign-in.

use reqwest::Client;
use serde_json::Value;

use super::Grant;
use crate::http::{self, Resp};
use crate::model::{Credential, Format, MetricLine, ProviderError, Result, Usage};

/// The Grok CLI's public OAuth client.
pub const CLIENT_ID: &str = "b1a00492-073a-47ea-816f-4c329264a828";
pub const DEVICE_CODE_URL: &str = "https://auth.x.ai/oauth2/device/code";
pub const TOKEN_URL: &str = "https://auth.x.ai/oauth2/token";
/// Enough to read usage. Without `api:access` the token cannot run Grok models.
pub const SCOPES: &str = "openid email offline_access grok-cli:access";
const BILLING_URL: &str = "https://cli-chat-proxy.grok.com/v1/billing?format=credits";
const SETTINGS_URL: &str = "https://cli-chat-proxy.grok.com/v1/settings";
const WEEKLY: &str = "USAGE_PERIOD_TYPE_WEEKLY";

pub async fn refresh(http: &Client, refresh_token: &str) -> Result<Grant> {
    let form = [("grant_type", "refresh_token"), ("client_id", CLIENT_ID), ("refresh_token", refresh_token)];
    Grant::from_renewal(&http::send(http.post(TOKEN_URL).form(&form)).await?)
}

async fn get(http: &Client, url: &str, token: &str) -> Result<Resp> {
    http::send(http.get(url).bearer_auth(token.trim()).header("X-XAI-Token-Auth", "xai-grok-cli").header("Accept", "application/json"))
        .await
}

pub async fn usage(http: &Client, credential: &Credential) -> Result<Usage> {
    let lines = map_billing(&get(http, BILLING_URL, credential.secret()).await?)?;
    // The plan name is a nicety: a failed read leaves the card without one.
    let plan = match get(http, SETTINGS_URL, credential.secret()).await {
        Ok(response) if response.is_ok() => response.json().and_then(|body| http::text(&body["subscription_tier_display"])),
        _ => None,
    };
    Ok(Usage { plan, lines })
}

pub fn map_billing(response: &Resp) -> Result<Vec<MetricLine>> {
    if response.status == 412 {
        let error = response.json().and_then(|body| http::text(&body["error"])).unwrap_or_default();
        if error.to_lowercase().contains("no personal team") {
            return Err(ProviderError::Unavailable("Grok team accounts have no personal quota.".into()));
        }
    }
    let body = response.require_json(false)?;
    let config: &Value = &body["config"];
    let period = &config["currentPeriod"];
    let (Some(kind), Some(start), Some(end)) =
        (http::text(&period["type"]), http::date_ms(&period["start"]), http::date_ms(&period["end"]))
    else {
        return Err(ProviderError::InvalidResponse);
    };
    if end <= start {
        return Err(ProviderError::InvalidResponse);
    }
    let mut lines = Vec::new();
    if kind == WEEKLY {
        let used = http::num(&config["creditUsagePercent"]).unwrap_or(0.0);
        lines.push(MetricLine::percent("Weekly", used, Some(end), Some(end - start)));
    }
    // The pay-as-you-go cap is in cents; no cap, or a zero one, means extra usage is off.
    if let Some(cap) = http::num(&config["onDemandCap"]["val"]).filter(|cap| *cap > 0.0) {
        lines.push(MetricLine::value("Extra Usage Cap", cap / 100.0, Format::dollars()));
    }
    if lines.is_empty() {
        return Err(ProviderError::Unavailable("Grok reports no usage for this account.".into()));
    }
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::WEEK_MS;
    use crate::providers::resp;
    use serde_json::json;

    #[test]
    fn maps_the_weekly_limit_and_the_cap() {
        let lines = map_billing(&resp(
            200,
            json!({"config": {
                "currentPeriod": {"type": "USAGE_PERIOD_TYPE_WEEKLY", "start": "2026-01-01T00:00:00Z", "end": "2026-01-08T00:00:00Z"},
                "creditUsagePercent": 37.5,
                "onDemandCap": {"val": 2500}
            }}),
        ))
        .unwrap();
        assert_eq!(lines[0], MetricLine::percent("Weekly", 37.5, Some(1_767_830_400_000), Some(WEEK_MS)));
        assert_eq!(lines[1], MetricLine::value("Extra Usage Cap", 25.0, Format::dollars()));
    }

    #[test]
    fn a_team_account_and_a_malformed_answer_are_told_apart() {
        let team = resp(412, json!({"error": "No personal team found"}));
        assert!(matches!(map_billing(&team), Err(ProviderError::Unavailable(_))));
        assert!(matches!(map_billing(&resp(200, json!({"config": {}}))), Err(ProviderError::InvalidResponse)));
        assert!(matches!(map_billing(&resp(401, json!({}))), Err(ProviderError::SignInExpired)));
    }
}
