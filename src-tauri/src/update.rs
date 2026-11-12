//! Checks GitHub for a newer release. The check sends nothing about the user: it is one anonymous
//! read of the public release list.

use std::cmp::Ordering;

use reqwest::Client;
use serde::Serialize;

use crate::http;

const RELEASES_URL: &str = "https://api.github.com/repos/KRSLYS/AI-Balance/releases?per_page=20";

#[derive(Serialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Update {
    pub version: String,
    pub url: String,
}

/// A version such as `1.2.3` or `1.2.3-beta.4`, ordered the way semantic versioning orders them.
#[derive(PartialEq, Eq, Debug)]
struct Version {
    core: [u64; 3],
    /// Empty for a final release, which sorts after every pre-release of the same core.
    pre: Vec<String>,
}

impl Version {
    fn parse(text: &str) -> Option<Self> {
        let text = text.trim().trim_start_matches('v');
        let (core, pre) = text.split_once('-').unwrap_or((text, ""));
        let mut parts = core.split('.').map(|part| part.parse::<u64>().ok());
        let core = [parts.next()??, parts.next()??, parts.next()??];
        if parts.next().is_some() {
            return None;
        }
        Some(Self { core, pre: pre.split('.').filter(|part| !part.is_empty()).map(str::to_string).collect() })
    }

    fn is_prerelease(&self) -> bool {
        !self.pre.is_empty()
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.core.cmp(&other.core).then_with(|| match (self.pre.is_empty(), other.pre.is_empty()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            (false, false) => {
                for (a, b) in self.pre.iter().zip(&other.pre) {
                    let order = match (a.parse::<u64>(), b.parse::<u64>()) {
                        (Ok(a), Ok(b)) => a.cmp(&b),
                        (Ok(_), Err(_)) => Ordering::Less,
                        (Err(_), Ok(_)) => Ordering::Greater,
                        (Err(_), Err(_)) => a.cmp(b),
                    };
                    if order != Ordering::Equal {
                        return order;
                    }
                }
                self.pre.len().cmp(&other.pre.len())
            }
        })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// The newest release that is newer than `current`. A final release is never offered a
/// pre-release; a pre-release is offered both.
pub fn newest<'a>(current: &str, releases: impl Iterator<Item = (&'a str, &'a str)>) -> Option<Update> {
    let current = Version::parse(current)?;
    releases
        .filter_map(|(tag, url)| Some((Version::parse(tag)?, tag, url)))
        .filter(|(version, _, _)| *version > current && (current.is_prerelease() || !version.is_prerelease()))
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, tag, url)| Update { version: tag.trim_start_matches('v').to_string(), url: url.to_string() })
}

/// `None` when there is nothing newer, and also when the check could not be made.
pub async fn check(http: &Client, current: &str) -> Option<Update> {
    let request = http.get(RELEASES_URL).header("Accept", "application/vnd.github+json");
    let body = http::send(request).await.ok()?.require_json(false).ok()?;
    let releases = body.as_array()?;
    newest(
        current,
        releases
            .iter()
            .filter(|release| release["draft"].as_bool() != Some(true))
            .filter_map(|release| Some((release["tag_name"].as_str()?, release["html_url"].as_str()?))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offer(current: &str, tags: &[&str]) -> Option<String> {
        newest(current, tags.iter().map(|tag| (*tag, "https://example.com"))).map(|update| update.version)
    }

    #[test]
    fn versions_order_like_semantic_versioning() {
        let order = ["0.1.0-alpha", "0.1.0-beta.2", "0.1.0-beta.10", "0.1.0", "0.1.1", "0.2.0", "1.0.0"];
        for pair in order.windows(2) {
            assert!(Version::parse(pair[0]).unwrap() < Version::parse(pair[1]).unwrap(), "{} < {}", pair[0], pair[1]);
        }
        assert_eq!(Version::parse("v1.2.3"), Version::parse("1.2.3"));
        assert_eq!(Version::parse("1.2"), None);
        assert_eq!(Version::parse("latest"), None);
    }

    #[test]
    fn a_final_release_is_not_offered_a_pre_release() {
        assert_eq!(offer("0.1.0", &["v0.2.0-beta.1", "v0.1.0"]), None);
        assert_eq!(offer("0.1.0", &["v0.2.0-beta.1", "v0.1.1", "v0.1.0"]).as_deref(), Some("0.1.1"));
    }

    #[test]
    fn a_pre_release_is_offered_the_newest_of_either_kind() {
        assert_eq!(offer("0.1.0-beta.4", &["v0.1.0-beta.5", "v0.1.0-beta.4", "nightly"]).as_deref(), Some("0.1.0-beta.5"));
        assert_eq!(offer("0.1.0-beta.4", &["v0.1.0-beta.5", "v0.1.0"]).as_deref(), Some("0.1.0"));
        assert_eq!(offer("0.1.0-beta.4", &["v0.1.0-beta.3"]), None);
    }
}
