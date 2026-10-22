//! Narrow URL boundaries shared by IPC transport, browser actions and deep links.
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use url::{Host, Url};
use uuid::Uuid;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ServerOrigin(String);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvalidLink;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentLinkRequest {
    pub href: String,
}
impl DocumentLinkRequest {
    pub fn checked(&self) -> Result<&str, InvalidLink> {
        let input = &self.href;
        if input.len() > 2048
            || input.chars().any(|c| c.is_control() || c.is_whitespace())
            || input.contains('\\')
        {
            return Err(InvalidLink);
        }
        let url = Url::parse(input).map_err(|_| InvalidLink)?;
        if !url.username().is_empty() || url.password().is_some() {
            return Err(InvalidLink);
        }
        match url.scheme() {
            "http" | "https" if url.host_str().is_some() => Ok(input),
            "mailto"
                if !url.path().is_empty() && url.query().is_none() && url.fragment().is_none() =>
            {
                Ok(input)
            }
            _ => Err(InvalidLink),
        }
    }
}
impl ServerOrigin {
    pub fn parse(input: &str) -> Result<Self, InvalidLink> {
        if input.is_empty()
            || input.len() > 2048
            || input.chars().any(char::is_control)
            || input.trim() != input
            || input.contains('\\')
        {
            return Err(InvalidLink);
        }
        let authority_and_path = input.split_once("://").ok_or(InvalidLink)?.1;
        let raw_authority = authority_and_path
            .split(['/', '?', '#'])
            .next()
            .ok_or(InvalidLink)?;
        let suffix = &authority_and_path[raw_authority.len()..];
        if !suffix.is_empty() && suffix != "/" {
            return Err(InvalidLink);
        }
        let url = Url::parse(input).map_err(|_| InvalidLink)?;
        if matches!(url.host(), Some(Host::Ipv4(_) | Host::Ipv6(_))) {
            let raw_host = if raw_authority.starts_with('[') {
                let end = raw_authority.find(']').ok_or(InvalidLink)?;
                &raw_authority[..=end]
            } else {
                raw_authority.split(':').next().ok_or(InvalidLink)?
            };
            if Some(raw_host) != url.host_str() {
                return Err(InvalidLink);
            }
        }
        if !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.path() != "/"
            || url.host().is_none()
        {
            return Err(InvalidLink);
        }
        let loopback = match url.host() {
            Some(Host::Ipv4(address)) => IpAddr::V4(address).is_loopback(),
            Some(Host::Ipv6(address)) => IpAddr::V6(address).is_loopback(),
            _ => false,
        };
        if url.scheme() != "https" && !(url.scheme() == "http" && loopback) {
            return Err(InvalidLink);
        }
        Ok(Self(url.origin().ascii_serialization()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn browser_action(&self, action: BrowserAction) -> String {
        let path = match action {
            BrowserAction::Setup => "/",
            BrowserAction::Join => "/join",
            BrowserAction::Recover => "/recover",
        };
        format!("{}{path}", self.0)
    }
}
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserAction {
    Setup,
    Join,
    Recover,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IssueLink {
    pub server: ServerOrigin,
    pub workspace: Uuid,
    pub issue: Uuid,
}
impl IssueLink {
    pub fn parse(input: &str) -> Result<Self, InvalidLink> {
        if input.len() > 4096 || input.chars().any(char::is_control) || input.contains('\\') {
            return Err(InvalidLink);
        }
        let url = Url::parse(input).map_err(|_| InvalidLink)?;
        if url.scheme() != "opentrack"
            || url.host_str() != Some("open")
            || !url.path().is_empty()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
            || url.fragment().is_some()
        {
            return Err(InvalidLink);
        }
        let mut server = None;
        let mut workspace = None;
        let mut issue = None;
        for (key, value) in url.query_pairs() {
            match key.as_ref() {
                "server" if server.is_none() => server = Some(ServerOrigin::parse(&value)?),
                "workspace" if workspace.is_none() => workspace = Some(canonical_uuid(&value)?),
                "issue" if issue.is_none() => issue = Some(canonical_uuid(&value)?),
                _ => return Err(InvalidLink),
            }
        }
        Ok(Self {
            server: server.ok_or(InvalidLink)?,
            workspace: workspace.ok_or(InvalidLink)?,
            issue: issue.ok_or(InvalidLink)?,
        })
    }
}
fn canonical_uuid(input: &str) -> Result<Uuid, InvalidLink> {
    let id = Uuid::parse_str(input).map_err(|_| InvalidLink)?;
    if id.to_string() != input {
        return Err(InvalidLink);
    }
    Ok(id)
}
/// Only packaged documents may navigate in a privileged webview. External links
/// must use a separate checked system-browser action, never this permission.
pub fn bundled_navigation(url: &Url) -> bool {
    url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && ((url.scheme() == "tauri" && url.host_str() == Some("localhost"))
            || (url.scheme() == "http" && url.host_str() == Some("tauri.localhost")))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn document_links_are_explicit_safe_destinations() {
        for href in [
            "https://example.com/path?q=1#part",
            "http://127.0.0.1:18970/",
            "mailto:team@example.com",
        ] {
            assert!(DocumentLinkRequest { href: href.into() }.checked().is_ok());
        }
        for href in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "data:text/html,x",
            "opentrack://open",
            "https://user:password@example.com",
            "https://example.com\\@bad.test",
            "mailto:team@example.com?body=secret",
            "https://example.com\n",
            "--help",
        ] {
            assert!(
                DocumentLinkRequest { href: href.into() }.checked().is_err(),
                "Accepted {href}"
            );
        }
    }
    #[test]
    fn origins_are_canonical_and_loopback_http_is_explicit() {
        assert_eq!(
            ServerOrigin::parse("https://EXAMPLE.com:443/")
                .unwrap()
                .as_str(),
            "https://example.com"
        );
        assert_eq!(
            ServerOrigin::parse("http://127.0.0.1:18970")
                .unwrap()
                .as_str(),
            "http://127.0.0.1:18970"
        );
        assert_eq!(
            ServerOrigin::parse("http://[::1]:18970").unwrap().as_str(),
            "http://[::1]:18970"
        );
        for bad in [
            "http://localhost",
            "http://127.1",
            "http://2130706433",
            "http://0x7f000001",
            "http://[0:0:0:0:0:0:0:1]",
            "https://example.com/path/..",
            "https://example.com/%2e%2e",
            "http://example.com",
            "http://127.0.0.1.evil.test",
            "https://user@example.com",
            "https://example.com/path",
            "https://example.com/?token=x",
            "https://example.com/#x",
            " https://example.com",
            "file:///etc/passwd",
            "https://example.com\\@evil.test",
        ] {
            assert!(ServerOrigin::parse(bad).is_err(), "Accepted {bad}");
        }
    }
    #[test]
    fn browser_actions_cannot_choose_paths_or_queries() {
        let origin = ServerOrigin::parse("https://example.com").unwrap();
        assert_eq!(
            origin.browser_action(BrowserAction::Setup),
            "https://example.com/"
        );
        assert_eq!(
            origin.browser_action(BrowserAction::Join),
            "https://example.com/join"
        );
        assert_eq!(
            origin.browser_action(BrowserAction::Recover),
            "https://example.com/recover"
        );
        assert!(serde_json::from_str::<BrowserAction>("\"https://evil.test\"").is_err());
    }
    #[test]
    fn issue_links_require_exact_keys_and_no_credentials() {
        let base = "opentrack://open?server=https%3A%2F%2Fexample.com&workspace=00000000-0000-0000-0000-000000000001&issue=00000000-0000-0000-0000-000000000002";
        let link = IssueLink::parse(base).unwrap();
        assert_eq!(link.server.as_str(), "https://example.com");
        for bad in [
            format!("{base}&issue=00000000-0000-0000-0000-000000000003"),
            format!("{base}&token=secret"),
            format!("{base}#fragment"),
            base.replace("//open?", "//open/path?"),
            base.replace("https%3A%2F%2Fexample.com", "file%3A%2F%2F%2Fetc%2Fpasswd"),
        ] {
            assert!(IssueLink::parse(&bad).is_err());
        }
    }
    #[test]
    fn privileged_navigation_rejects_remote_and_lookalike_origins() {
        for good in [
            "tauri://localhost/index.html",
            "http://tauri.localhost/index.html",
        ] {
            assert!(bundled_navigation(&Url::parse(good).unwrap()));
        }
        for bad in [
            "https://example.com",
            "tauri://localhost.evil/index.html",
            "http://tauri.localhost:8080/",
            "http://user@tauri.localhost/",
            "file:///tmp/index.html",
            "data:text/html,test",
        ] {
            assert!(!bundled_navigation(&Url::parse(bad).unwrap()));
        }
    }
}
