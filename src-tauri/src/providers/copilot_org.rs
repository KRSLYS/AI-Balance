//! Copilot for an organization: the credits and spend GitHub bills an organization this month.
//!
//! Read with a fine-grained token an owner or billing manager creates for this purpose
//! (Organization permissions, Administration, read-only). A personal Copilot sign-in cannot read it.

use reqwest::Client;

use crate::http::{self, Resp};
use crate::model::{Credential, Format, MetricLine, ProviderError, Result, Usage};

pub const ORGANIZATION_KEY: &str = "organization";

/// An organization's login as GitHub allows it: letters, digits and single hyphens, at most 39
/// characters. It goes into a request path, so nothing else is accepted.
pub fn valid_organization(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 39
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        && !name.starts_with('-')
        && !name.ends_with('-')
        && !name.contains("--")
}

pub async fn usage(http: &Client, credential: &Credential) -> Result<Usage> {
    let organization = credential.extra(ORGANIZATION_KEY).filter(|name| valid_organization(name)).ok_or(ProviderError::InvalidResponse)?;
    let request = http
        .get(format!("https://api.github.com/organizations/{organization}/settings/billing/ai_credit/usage"))
        .bearer_auth(credential.secret())
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28");
    Ok(Usage { plan: None, lines: map_usage(&http::send(request).await?, organization)? })
}

pub fn map_usage(response: &Resp, organization: &str) -> Result<Vec<MetricLine>> {
    match response.status {
        401 | 403 => {
            return Err(ProviderError::KeyRejected("GitHub rejected the token, or it cannot read this organization's billing.".into()))
        }
        404 => return Err(ProviderError::Unavailable(format!("GitHub has no organization “{organization}” this token can read."))),
        _ => {}
    }
    let body = response.require_json(true)?;
    let items = body["usageItems"].as_array().ok_or(ProviderError::InvalidResponse)?;
    let (mut credits, mut spend) = (0.0, 0.0);
    for item in items.iter().filter(|item| item["product"].as_str() == Some("Copilot")) {
        credits += http::num(&item["grossQuantity"]).ok_or(ProviderError::InvalidResponse)?;
        spend += http::num(&item["netAmount"]).ok_or(ProviderError::InvalidResponse)?;
    }
    Ok(vec![MetricLine::value("Org Credits", credits, Format::count("credits")), MetricLine::value("Org Spend", spend, Format::dollars())])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::resp;
    use serde_json::json;

    #[test]
    fn adds_up_the_copilot_items_only() {
        let lines = map_usage(
            &resp(
                200,
                json!({"usageItems": [
                    {"product": "Copilot", "grossQuantity": 120, "netAmount": 4.5},
                    {"product": "Copilot", "grossQuantity": 30.5, "netAmount": 0},
                    {"product": "Actions", "grossQuantity": 999, "netAmount": 999}
                ]}),
            ),
            "acme",
        )
        .unwrap();
        assert_eq!(lines[0], MetricLine::value("Org Credits", 150.5, Format::count("credits")));
        assert_eq!(lines[1], MetricLine::value("Org Spend", 4.5, Format::dollars()));
    }

    #[test]
    fn refusals_are_told_apart() {
        assert!(matches!(map_usage(&resp(403, json!({})), "acme"), Err(ProviderError::KeyRejected(_))));
        assert!(matches!(map_usage(&resp(404, json!({})), "acme"), Err(ProviderError::Unavailable(_))));
        assert!(matches!(map_usage(&resp(200, json!({})), "acme"), Err(ProviderError::InvalidResponse)));
    }

    #[test]
    fn organization_names_cannot_reach_another_path() {
        for good in ["acme", "Acme-Corp", "a1"] {
            assert!(valid_organization(good), "{good}");
        }
        for bad in ["", "-acme", "acme-", "a--b", "acme/../user", "acme?x=1", "acme corp", &"a".repeat(40)] {
            assert!(!valid_organization(bad), "{bad}");
        }
    }
}
