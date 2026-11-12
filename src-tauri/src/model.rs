//! Data types shared by the providers, the store and the UI.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const SESSION_MS: i64 = 5 * 60 * 60 * 1000;
pub const DAY_MS: i64 = 24 * 60 * 60 * 1000;
pub const WEEK_MS: i64 = 7 * DAY_MS;
pub const MONTH_MS: i64 = 30 * DAY_MS;

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    Claude,
    Codex,
    Copilot,
    Cursor,
    Kimi,
    Grok,
    Amp,
    OpenCode,
    Muse,
    Devin,
    CopilotOrg,
    OpenRouter,
    Zai,
    Ollama,
}

impl ProviderKind {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Claude => "Claude",
            Self::Codex => "ChatGPT",
            Self::Copilot => "Copilot",
            Self::Cursor => "Cursor",
            Self::Kimi => "Kimi",
            Self::Grok => "Grok",
            Self::Amp => "Amp",
            Self::OpenCode => "OpenCode",
            Self::Muse => "Muse Code",
            Self::Devin => "Devin",
            Self::CopilotOrg => "Copilot Organization",
            Self::OpenRouter => "OpenRouter",
            Self::Zai => "Z.ai",
            Self::Ollama => "Ollama",
        }
    }

    pub fn uses_api_key(self) -> bool {
        matches!(self, Self::OpenRouter | Self::Zai | Self::Ollama | Self::CopilotOrg)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub provider: ProviderKind,
    pub label: String,
    pub created_at: i64,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Credential {
    ApiKey {
        key: String,
    },
    OAuth {
        access_token: String,
        #[serde(default)]
        refresh_token: Option<String>,
        /// Unix milliseconds.
        #[serde(default)]
        expires_at: Option<i64>,
        #[serde(default)]
        extras: BTreeMap<String, String>,
    },
}

impl Credential {
    pub fn oauth(access_token: String, refresh_token: Option<String>, expires_at: Option<i64>) -> Self {
        Self::OAuth { access_token, refresh_token, expires_at, extras: BTreeMap::new() }
    }

    pub fn with_extra(mut self, key: &str, value: Option<String>) -> Self {
        if let (Self::OAuth { extras, .. }, Some(value)) = (&mut self, value) {
            extras.insert(key.to_string(), value);
        }
        self
    }

    pub fn extra(&self, key: &str) -> Option<&str> {
        match self {
            Self::OAuth { extras, .. } => extras.get(key).map(String::as_str),
            Self::ApiKey { .. } => None,
        }
    }

    /// The API key or the OAuth access token.
    pub fn secret(&self) -> &str {
        match self {
            Self::ApiKey { key } => key,
            Self::OAuth { access_token, .. } => access_token,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Format {
    Percent,
    Currency { code: String },
    Count { suffix: String },
}

impl Format {
    pub fn dollars() -> Self {
        Self::Currency { code: "USD".into() }
    }

    pub fn count(suffix: &str) -> Self {
        Self::Count { suffix: suffix.into() }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum MetricLine {
    /// A limit with a bar: `used` out of `limit`.
    Progress { label: String, used: f64, limit: f64, format: Format, resets_at: Option<i64>, period_ms: Option<i64> },
    /// A plain number, such as a balance.
    Value { label: String, value: f64, format: Format },
}

impl MetricLine {
    pub fn percent(label: &str, used: f64, resets_at: Option<i64>, period_ms: Option<i64>) -> Self {
        Self::Progress { label: label.into(), used: used.clamp(0.0, 100.0), limit: 100.0, format: Format::Percent, resets_at, period_ms }
    }

    pub fn progress(label: &str, used: f64, limit: f64, format: Format, resets_at: Option<i64>, period_ms: Option<i64>) -> Self {
        Self::Progress { label: label.into(), used, limit, format, resets_at, period_ms }
    }

    pub fn value(label: &str, value: f64, format: Format) -> Self {
        Self::Value { label: label.into(), value, format }
    }

    pub fn label(&self) -> &str {
        match self {
            Self::Progress { label, .. } | Self::Value { label, .. } => label,
        }
    }

    /// Used share of a limit, 0–100. `None` for plain values and limits of zero.
    pub fn used_percent(&self) -> Option<f64> {
        match self {
            Self::Progress { used, limit, .. } if *limit > 0.0 => Some((used / limit * 100.0).clamp(0.0, 100.0)),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Usage {
    pub plan: Option<String>,
    pub lines: Vec<MetricLine>,
}

/// The last thing known about an account: its usage, or why it could not be read.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub account_id: String,
    pub plan: Option<String>,
    pub lines: Vec<MetricLine>,
    /// When the usage in `lines` was read, Unix milliseconds.
    pub fetched_at: Option<i64>,
    pub error: Option<String>,
    pub needs_login: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub refresh_minutes: u32,
    pub notifications: bool,
    /// Notify when a limit's used share reaches this percentage.
    pub notify_threshold: u32,
    pub language: String,
    pub theme: String,
    /// The first-run welcome screen has been completed.
    pub onboarded: bool,
    /// Look for a newer release now and then.
    pub check_updates: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            refresh_minutes: 5,
            // Off until the user turns it on, which is when the system is asked for permission.
            notifications: false,
            notify_threshold: 90,
            language: "system".into(),
            theme: "system".into(),
            onboarded: false,
            check_updates: true,
        }
    }
}

impl Settings {
    pub fn sanitized(mut self) -> Self {
        self.refresh_minutes = self.refresh_minutes.clamp(2, 120);
        self.notify_threshold = self.notify_threshold.clamp(50, 100);
        self
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("Sign-in expired. Sign in again.")]
    SignInExpired,
    #[error("{0}")]
    KeyRejected(String),
    #[error("Rate limited by the provider. Trying again later.")]
    RateLimited { retry_after_secs: Option<u64> },
    #[error("{0}")]
    Unavailable(String),
    #[error("The provider answered with HTTP {0}.")]
    Http(u16),
    #[error("Could not reach the provider. Check your connection.")]
    Network,
    #[error("The provider sent a response this version does not understand.")]
    InvalidResponse,
}

impl ProviderError {
    pub fn needs_login(&self) -> bool {
        matches!(self, Self::SignInExpired | Self::KeyRejected(_))
    }
}

impl From<reqwest::Error> for ProviderError {
    fn from(_: reqwest::Error) -> Self {
        Self::Network
    }
}

pub type Result<T> = std::result::Result<T, ProviderError>;
