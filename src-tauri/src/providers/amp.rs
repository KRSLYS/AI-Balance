//! Amp: what `amp usage` prints for an Amp sign-in.

use regex_lite::Regex;
use reqwest::Client;
use serde_json::json;

use super::Grant;
use crate::http::{self, Resp};
use crate::model::{Credential, Format, MetricLine, ProviderError, Result, Usage, DAY_MS};

/// The Amp CLI's public client.
pub const CLIENT_ID: &str = "client_01JNSEYM3V0J5AXK4YXNXRXTGP";
pub const DEVICE_CODE_URL: &str = "https://authapi.ampcode.com/user_management/authorize/device";
pub const TOKEN_URL: &str = "https://authapi.ampcode.com/user_management/authenticate";
/// The page `amp login` opens. It takes the code in its address.
pub const VERIFICATION_PAGE: &str = "https://ampcode.com/auth/cli/device";
const BALANCE_URL: &str = "https://ampcode.com/api/internal?userDisplayBalanceInfo";

pub async fn refresh(http: &Client, refresh_token: &str) -> Result<Grant> {
    let body = json!({"client_id": CLIENT_ID, "grant_type": "refresh_token", "refresh_token": refresh_token});
    let mut grant = Grant::from_renewal(&http::send(http.post(TOKEN_URL).json(&body)).await?)?;
    grant.expires_at = http::jwt_expiry_ms(&grant.access_token).or(grant.expires_at);
    Ok(grant)
}

pub async fn balance(http: &Client, token: &str) -> Result<Resp> {
    let body = json!({"method": "userDisplayBalanceInfo", "params": {}});
    http::send(http.post(BALANCE_URL).bearer_auth(token).header("Accept", "application/json").json(&body)).await
}

pub async fn usage(http: &Client, credential: &Credential) -> Result<Usage> {
    let response = balance(http, credential.secret()).await?;
    map_balance(&display_text(&response)?)
}

/// The text Amp shows for the account, without its bold markers.
pub fn display_text(response: &Resp) -> Result<String> {
    let body = response.require_json(false)?;
    http::text(&body["result"]["displayText"]).map(|text| text.replace("**", "")).ok_or(ProviderError::InvalidResponse)
}

pub fn email(text: &str) -> Option<String> {
    Some(pattern(r"Signed in as ([^\s(]+)").captures(text)?[1].to_string())
}

fn pattern(source: &str) -> Regex {
    Regex::new(source).expect("static pattern")
}

fn amount(text: &str) -> Option<f64> {
    text.replace(',', "").parse().ok()
}

/// Amp reports its numbers as sentences, so the card is read out of them.
pub fn map_balance(text: &str) -> Result<Usage> {
    let mut plan = None;
    let mut lines = Vec::new();

    let tier = pattern(r"Amp ([^\n]+?) Tier: agent usage \$([0-9.,]+) of \$([0-9.,]+) remaining([^\n]*)");
    if let Some(found) = tier.captures(text) {
        if let (Some(left), Some(limit)) = (amount(&found[2]), amount(&found[3]).filter(|limit| *limit > 0.0)) {
            plan = Some(found[1].to_string());
            let rest = &found[4];
            let period = pattern(r"period (\d{4}-\d{2}-\d{2}) to (\d{4}-\d{2}-\d{2})").captures(rest).and_then(|days| {
                let start = http::date_ms(&json!(&days[1]))?;
                let end = http::date_ms(&json!(&days[2]))?;
                (end > start).then_some((start, end))
            });
            let (resets_at, period_ms) = (period.map(|(_, end)| end), period.map(|(start, end)| end - start));
            let used = ((limit - left) * 100.0).round() / 100.0;
            lines.push(MetricLine::progress("Agent Usage", used.max(0.0), limit, Format::dollars(), resets_at, period_ms));
            if let Some(orb) = pattern(r"orb usage ([0-9.,]+)h of ([0-9.,]+)h").captures(rest) {
                if let (Some(left), Some(limit)) = (amount(&orb[1]), amount(&orb[2]).filter(|limit| *limit > 0.0)) {
                    let used = ((limit - left) * 10.0).round() / 10.0;
                    lines.push(MetricLine::progress("Orb Hours", used.max(0.0), limit, Format::count("hours"), resets_at, period_ms));
                }
            }
        }
    }

    let free_dollars = pattern(r"Amp Free: \$([0-9.,]+)\s*/\s*\$([0-9.,]+) remaining").captures(text);
    let free_percent = pattern(r"Amp Free: ([0-9.]+)% remaining").captures(text);
    let free_used = match (free_dollars, free_percent) {
        (Some(found), _) => match (amount(&found[1]), amount(&found[2]).filter(|limit| *limit > 0.0)) {
            (Some(left), Some(limit)) => Some((limit - left) / limit * 100.0),
            _ => None,
        },
        (None, Some(found)) => amount(&found[1]).map(|left| 100.0 - left),
        (None, None) => None,
    };
    if let Some(used) = free_used {
        lines.push(MetricLine::percent("Amp Free", used, None, Some(DAY_MS)));
        plan = plan.or_else(|| Some("Free".into()));
    }

    if let Some(left) = pattern(r"Individual credits: \$([0-9.,]+) remaining").captures(text).and_then(|found| amount(&found[1])) {
        lines.push(MetricLine::value("Individual Credits", left, Format::dollars()));
    }
    if lines.is_empty() {
        return Err(ProviderError::InvalidResponse);
    }
    Ok(Usage { plan, lines })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_tier_orb_hours_free_allowance_and_credits() {
        let text = "Signed in as dev@example.com (dev)\n\
            Amp Pro Tier: agent usage $12.50 of $20.00 remaining, orb usage 7.5h of 10h remaining, period 2026-01-01 to 2026-01-31\n\
            Amp Free: $2.00 / $10.00 remaining\n\
            Individual credits: $1,234.50 remaining";
        let usage = map_balance(text).unwrap();
        assert_eq!(usage.plan.as_deref(), Some("Pro"));
        assert_eq!(
            usage.lines[0],
            MetricLine::progress("Agent Usage", 7.5, 20.0, Format::dollars(), Some(1_769_817_600_000), Some(30 * DAY_MS))
        );
        assert_eq!(usage.lines[1].label(), "Orb Hours");
        assert_eq!(usage.lines[1].used_percent(), Some(25.0));
        assert_eq!(usage.lines[2], MetricLine::percent("Amp Free", 80.0, None, Some(DAY_MS)));
        assert_eq!(usage.lines[3], MetricLine::value("Individual Credits", 1234.5, Format::dollars()));
        assert_eq!(email(text).as_deref(), Some("dev@example.com"));
    }

    #[test]
    fn a_free_account_reports_a_percentage() {
        let usage = map_balance("Amp Free: 65% remaining").unwrap();
        assert_eq!(usage.plan.as_deref(), Some("Free"));
        assert_eq!(usage.lines[0].used_percent(), Some(35.0));
    }

    #[test]
    fn text_without_numbers_is_not_understood() {
        assert!(matches!(map_balance("Welcome to Amp"), Err(ProviderError::InvalidResponse)));
    }
}
