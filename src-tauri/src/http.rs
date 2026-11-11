//! A thin layer over reqwest plus the JSON helpers every provider needs.

use std::time::Duration;

use base64::Engine;
use reqwest::header::HeaderMap;
use serde_json::Value;

use crate::model::{ProviderError, Result};

pub const USER_AGENT: &str = concat!("AIBalance/", env!("CARGO_PKG_VERSION"));

pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(20))
        .connect_timeout(Duration::from_secs(10))
        .build()
        .expect("the HTTP client builds with static options")
}

pub struct Resp {
    pub status: u16,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
}

impl Resp {
    pub fn is_ok(&self) -> bool {
        (200..300).contains(&self.status)
    }

    pub fn json(&self) -> Option<Value> {
        serde_json::from_slice(&self.body).ok()
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|v| v.to_str().ok())
    }

    fn retry_after_secs(&self) -> Option<u64> {
        self.header("retry-after").and_then(|v| v.trim().parse().ok())
    }

    /// The JSON body of a successful response. `key_auth` picks the message for a 401 or 403:
    /// an API key was rejected, or an OAuth sign-in expired.
    pub fn require_json(&self, key_auth: bool) -> Result<Value> {
        self.require_ok(key_auth)?;
        self.json().ok_or(ProviderError::InvalidResponse)
    }

    pub fn require_ok(&self, key_auth: bool) -> Result<()> {
        match self.status {
            200..=299 => Ok(()),
            401 | 403 if key_auth => Err(ProviderError::KeyRejected("The provider rejected this API key.".into())),
            401 | 403 => Err(ProviderError::SignInExpired),
            429 => Err(ProviderError::RateLimited { retry_after_secs: self.retry_after_secs() }),
            status => Err(ProviderError::Http(status)),
        }
    }
}

pub async fn send(request: reqwest::RequestBuilder) -> Result<Resp> {
    let response = request.send().await?;
    let status = response.status().as_u16();
    let headers = response.headers().clone();
    let body = response.bytes().await?.to_vec();
    Ok(Resp { status, headers, body })
}

/// A JSON number, or a string holding one. Providers send both.
pub fn num(value: &Value) -> Option<f64> {
    let number = match value {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }?;
    number.is_finite().then_some(number)
}

pub fn boolean(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(b) => Some(*b),
        Value::String(s) => match s.trim().to_ascii_lowercase().as_str() {
            "true" | "1" => Some(true),
            "false" | "0" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

pub fn text(value: &Value) -> Option<String> {
    let trimmed = value.as_str()?.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// Unix milliseconds from an ISO 8601 string, a `YYYY-MM-DD` day, or an epoch in seconds or milliseconds.
pub fn date_ms(value: &Value) -> Option<i64> {
    if let Some(text) = value.as_str() {
        let text = text.trim();
        if let Ok(date) = chrono::DateTime::parse_from_rfc3339(text) {
            return Some(date.timestamp_millis());
        }
        if let Ok(day) = chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d") {
            return Some(day.and_hms_opt(0, 0, 0)?.and_utc().timestamp_millis());
        }
    }
    let number = num(value)?;
    Some(if number.abs() < 1e11 { (number * 1000.0) as i64 } else { number as i64 })
}

pub fn jwt_payload(token: &str) -> Option<Value> {
    let payload = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub fn jwt_expiry_ms(token: &str) -> Option<i64> {
    let seconds = num(jwt_payload(token)?.get("exp")?)?;
    Some((seconds * 1000.0) as i64)
}

/// `pro_plus` → `Pro Plus`.
pub fn title_case(raw: &str) -> String {
    raw.split(|c: char| c == '_' || c == '-' || c.is_whitespace())
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            let first = chars.next().map(|c| c.to_uppercase().collect::<String>()).unwrap_or_default();
            first + &chars.as_str().to_lowercase()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn numbers_come_from_numbers_and_strings() {
        assert_eq!(num(&json!(4.5)), Some(4.5));
        assert_eq!(num(&json!(" 12 ")), Some(12.0));
        assert_eq!(num(&json!(null)), None);
        assert_eq!(num(&json!("abc")), None);
    }

    #[test]
    fn dates_parse_from_every_shape() {
        assert_eq!(date_ms(&json!("2026-01-02T03:04:05Z")), Some(1_767_323_045_000));
        assert_eq!(date_ms(&json!("2026-01-02")), Some(1_767_312_000_000));
        assert_eq!(date_ms(&json!(1_767_323_045)), Some(1_767_323_045_000));
        assert_eq!(date_ms(&json!(1_767_323_045_000_i64)), Some(1_767_323_045_000));
        assert_eq!(date_ms(&json!("soon")), None);
    }

    #[test]
    fn jwt_payload_and_expiry_decode() {
        // {"alg":"none"} . {"sub":"auth0|user_1","exp":1767323045}
        let token = "eyJhbGciOiJub25lIn0.eyJzdWIiOiJhdXRoMHx1c2VyXzEiLCJleHAiOjE3NjczMjMwNDV9.";
        assert_eq!(jwt_payload(token).unwrap()["sub"], "auth0|user_1");
        assert_eq!(jwt_expiry_ms(token), Some(1_767_323_045_000));
        assert_eq!(jwt_payload("not-a-jwt"), None);
    }

    #[test]
    fn title_case_splits_on_separators() {
        assert_eq!(title_case("pro_plus"), "Pro Plus");
        assert_eq!(title_case("FREE"), "Free");
        assert_eq!(title_case("max-20x plan"), "Max 20x Plan");
    }
}
