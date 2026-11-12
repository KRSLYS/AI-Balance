//! One module per provider. Each reads usage with a credential and maps the answer to metric lines.

pub mod amp;
pub mod claude;
pub mod claude_api;
pub mod codex;
pub mod copilot;
pub mod copilot_org;
pub mod cursor;
pub mod devin;
pub mod grok;
pub mod kimi;
pub mod muse;
pub mod ollama;
pub mod opencode;
pub mod openrouter;
pub mod zai;

use reqwest::Client;
use serde_json::Value;

use crate::http::{self, Resp};
use crate::model::{now_ms, Credential, ProviderError, ProviderKind, Result, Usage, DAY_MS};

/// A renewed OAuth sign-in.
pub struct Grant {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<i64>,
}

impl Grant {
    /// Reads a token endpoint's answer. Only a refusal of the refresh token itself ends the sign-in;
    /// an outage or a rate limit fails this pass and the next one renews again.
    pub fn from_renewal(response: &Resp) -> Result<Self> {
        let body = response.json();
        if matches!(response.status, 400 | 401 | 403) {
            let code = body.as_ref().and_then(error_code).unwrap_or_default();
            const ENDED: [&str; 4] = ["invalid_grant", "refresh_token_expired", "refresh_token_reused", "refresh_token_invalidated"];
            if ENDED.contains(&code.as_str()) {
                return Err(ProviderError::SignInExpired);
            }
        }
        response.require_ok(false)?;
        let body = body.ok_or(ProviderError::InvalidResponse)?;
        let access_token = http::text(&body["access_token"]).ok_or(ProviderError::InvalidResponse)?;
        let expires_at = http::num(&body["expires_in"])
            .map(|seconds| now_ms() + (seconds * 1000.0) as i64)
            .or_else(|| http::jwt_expiry_ms(&access_token));
        Ok(Self { access_token, refresh_token: http::text(&body["refresh_token"]), expires_at })
    }
}

/// RFC 6749 sends `error` as a string; some providers send `error.code` or a top-level `code`.
fn error_code(body: &Value) -> Option<String> {
    http::text(&body["error"]).or_else(|| http::text(&body["error"]["code"])).or_else(|| http::text(&body["code"]))
}

/// How long before its expiry a token is renewed.
fn renewal_lead_ms(kind: ProviderKind) -> i64 {
    match kind {
        // Cursor's token lasts 60 days and is its own refresh token, so it must be renewed while it still works.
        ProviderKind::Cursor => 7 * DAY_MS,
        _ => 5 * 60 * 1000,
    }
}

async fn renew(http: &Client, kind: ProviderKind, credential: &mut Credential) -> Result<()> {
    let Credential::OAuth { access_token, refresh_token, expires_at, extras } = credential else {
        return Err(ProviderError::SignInExpired);
    };
    let Some(token) = refresh_token.clone() else { return Err(ProviderError::SignInExpired) };
    let device_id = extras.get(kimi::DEVICE_ID_KEY).cloned().unwrap_or_default();
    let grant = match kind {
        ProviderKind::Claude => claude::refresh(http, &token).await?,
        ProviderKind::Codex => codex::refresh(http, &token).await?,
        ProviderKind::Cursor => cursor::refresh(http, &token).await?,
        ProviderKind::Kimi => kimi::refresh(http, &token, &device_id).await?,
        ProviderKind::Grok => grok::refresh(http, &token).await?,
        ProviderKind::Amp => amp::refresh(http, &token).await?,
        ProviderKind::OpenCode => opencode::refresh(http, &token).await?,
        _ => return Err(ProviderError::SignInExpired),
    };
    *access_token = grant.access_token;
    if grant.refresh_token.is_some() {
        *refresh_token = grant.refresh_token;
    }
    *expires_at = grant.expires_at;
    Ok(())
}

async fn read(http: &Client, kind: ProviderKind, credential: &mut Credential) -> Result<Usage> {
    match kind {
        ProviderKind::Claude => claude::usage(http, credential).await,
        ProviderKind::Codex => codex::usage(http, credential).await,
        ProviderKind::Copilot => copilot::usage(http, credential).await,
        ProviderKind::Cursor => cursor::usage(http, credential).await,
        ProviderKind::Kimi => kimi::usage(http, credential).await,
        ProviderKind::Grok => grok::usage(http, credential).await,
        ProviderKind::Amp => amp::usage(http, credential).await,
        ProviderKind::OpenCode => opencode::usage(http, credential).await,
        ProviderKind::Muse => muse::usage(http, credential).await,
        ProviderKind::Devin => devin::usage(http, credential).await,
        ProviderKind::CopilotOrg => copilot_org::usage(http, credential).await,
        ProviderKind::ClaudeApi => claude_api::usage(http, credential.secret()).await,
        ProviderKind::OpenRouter => openrouter::usage(http, credential.secret()).await,
        ProviderKind::Zai => zai::usage(http, credential.secret()).await,
        ProviderKind::Ollama => ollama::usage(http, credential.secret()).await,
    }
}

/// Reads an account's usage, renewing its sign-in first when it is about to expire and once more
/// when the provider refuses the token. The caller saves `credential` when it changed.
pub async fn fetch(http: &Client, kind: ProviderKind, credential: &mut Credential) -> Result<Usage> {
    let mut renewed = false;
    if let Credential::OAuth { refresh_token: Some(_), expires_at: Some(expires_at), .. } = credential {
        let expires_at = *expires_at;
        if expires_at - renewal_lead_ms(kind) <= now_ms() {
            match renew(http, kind, credential).await {
                Ok(()) => renewed = true,
                // The token still works: read with it and renew on a later pass.
                Err(error) if expires_at > now_ms() && !matches!(error, ProviderError::SignInExpired) => {}
                Err(error) => return Err(error),
            }
        }
    }
    match read(http, kind, credential).await {
        Err(ProviderError::SignInExpired) if !renewed && matches!(credential, Credential::OAuth { refresh_token: Some(_), .. }) => {
            renew(http, kind, credential).await?;
            read(http, kind, credential).await
        }
        result => result,
    }
}

#[cfg(test)]
pub(crate) fn resp(status: u16, body: Value) -> Resp {
    Resp { status, headers: Default::default(), body: serde_json::to_vec(&body).unwrap() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_refused_refresh_token_ends_the_sign_in() {
        let refused = resp(400, json!({"error": "invalid_grant"}));
        assert!(matches!(Grant::from_renewal(&refused), Err(ProviderError::SignInExpired)));
        let nested = resp(401, json!({"error": {"code": "refresh_token_reused"}}));
        assert!(matches!(Grant::from_renewal(&nested), Err(ProviderError::SignInExpired)));
    }

    #[test]
    fn an_outage_does_not_end_the_sign_in() {
        assert!(matches!(Grant::from_renewal(&resp(503, json!({}))), Err(ProviderError::Http(503))));
        assert!(matches!(Grant::from_renewal(&resp(429, json!({}))), Err(ProviderError::RateLimited { .. })));
    }

    #[test]
    fn a_renewal_reads_tokens_and_expiry() {
        let grant = Grant::from_renewal(&resp(200, json!({"access_token": "a", "refresh_token": "r", "expires_in": 3600}))).unwrap();
        assert_eq!(grant.access_token, "a");
        assert_eq!(grant.refresh_token.as_deref(), Some("r"));
        let left = grant.expires_at.unwrap() - now_ms();
        assert!((3_590_000..=3_600_000).contains(&left));
    }
}
