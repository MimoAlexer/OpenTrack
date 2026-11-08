use crate::ApiError;
use axum::{
    extract::{ConnectInfo, Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
};
use opentrack_domain::Error;
use std::{
    collections::{HashMap, HashSet},
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const WINDOW: Duration = Duration::from_secs(60);
const IP_LIMIT: u32 = 20;
const GLOBAL_LIMIT: u32 = 100;
const MAX_BUCKETS: usize = 1024;
#[derive(Default)]
pub(crate) struct Admission {
    state: Mutex<Buckets>,
    trusted_proxies: HashSet<IpAddr>,
}
#[derive(Default)]
struct Buckets {
    global: Option<Bucket>,
    sources: HashMap<IpAddr, Bucket>,
}
struct Bucket {
    start: Instant,
    count: u32,
}
impl Admission {
    pub(crate) fn new(trusted_proxies: Vec<IpAddr>) -> Self {
        Self {
            state: Mutex::default(),
            trusted_proxies: trusted_proxies.into_iter().map(normalize_ip).collect(),
        }
    }
    fn source(&self, request: &Request) -> Result<IpAddr, Error> {
        let Some(peer) = request.extensions().get::<ConnectInfo<SocketAddr>>() else {
            // In-process routers have no socket identity and cannot grant proxy
            // trust, even when the fallback address is explicitly trusted.
            return Ok(IpAddr::V4(Ipv4Addr::LOCALHOST));
        };
        let peer = normalize_ip(peer.0.ip());
        if !self.trusted_proxies.contains(&peer) {
            return Ok(peer);
        }
        let mut values = request.headers().get_all("x-opentrack-client-ip").iter();
        let source = values
            .next()
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<IpAddr>().ok());
        match (source.map(normalize_ip), values.next()) {
            (Some(source), None) if !source.is_unspecified() && !source.is_multicast() => {
                Ok(normalize_ip(source))
            }
            _ => Err(Error::Invalid(
                "A single proxy client IP address is required".into(),
            )),
        }
    }
    fn allow(&self, source: IpAddr, now: Instant) -> bool {
        let source = normalize_ip(source);
        let Ok(mut state) = self.state.lock() else {
            static REPORTED: std::sync::Once = std::sync::Once::new();
            REPORTED.call_once(|| {
                tracing::warn!(
                    auth_rejection = "state_unavailable",
                    "Authentication request rejected"
                )
            });
            return false;
        };
        state
            .sources
            .retain(|_, bucket| now.duration_since(bucket.start) < WINDOW);
        let global = state.global.get_or_insert(Bucket {
            start: now,
            count: 0,
        });
        if now.duration_since(global.start) >= WINDOW {
            *global = Bucket {
                start: now,
                count: 0,
            };
        }
        if global.count >= GLOBAL_LIMIT {
            static REPORTED: std::sync::Once = std::sync::Once::new();
            REPORTED.call_once(|| {
                tracing::warn!(
                    auth_rejection = "global_budget",
                    "Authentication request rejected"
                )
            });
            return false;
        }
        if state.sources.len() >= MAX_BUCKETS && !state.sources.contains_key(&source) {
            static REPORTED: std::sync::Once = std::sync::Once::new();
            REPORTED.call_once(|| {
                tracing::warn!(
                    auth_rejection = "source_capacity",
                    "Authentication request rejected"
                )
            });
            return false;
        }
        let bucket = state.sources.entry(source).or_insert(Bucket {
            start: now,
            count: 0,
        });
        if bucket.count >= IP_LIMIT {
            static REPORTED: std::sync::Once = std::sync::Once::new();
            REPORTED.call_once(|| {
                tracing::warn!(
                    auth_rejection = "peer_budget",
                    "Authentication request rejected"
                )
            });
            return false;
        }
        bucket.count += 1;
        // Charge admitted work only. A rejected source must not spend the
        // shared allowance reserved for other clients.
        state
            .global
            .as_mut()
            .expect("global bucket initialized")
            .count += 1;
        true
    }
}
fn normalize_ip(address: IpAddr) -> IpAddr {
    match address {
        IpAddr::V6(address) => address
            .to_ipv4_mapped()
            .map(IpAddr::V4)
            .unwrap_or(IpAddr::V6(address)),
        address => address,
    }
}
pub(crate) async fn limit_auth(
    State(state): State<Arc<Admission>>,
    request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path();
    let browser_auth = [
        "/auth/login",
        "/auth/recovery/complete",
        "/setup",
        "/invitations/preview",
        "/invitations/accept",
    ]
    .iter()
    .any(|suffix| path.ends_with(suffix));
    let native_auth = path.ends_with("/auth/native/requests")
        || path.ends_with("/auth/native/exchange")
        || (path.contains("/auth/native/requests/") && path.ends_with("/approve"));
    if request.method() == axum::http::Method::POST && (browser_auth || native_auth) {
        let source = match state.source(&request) {
            Ok(source) => source,
            Err(error) => return ApiError::from(error).into_response(),
        };
        if !state.allow(source, Instant::now()) {
            return ApiError::from(Error::RateLimited).into_response();
        }
    }
    next.run(request).await
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_limit_expires_without_growing_buckets() {
        let a = Admission::default();
        let now = Instant::now();
        let ip = IpAddr::V4(Ipv4Addr::LOCALHOST);
        for _ in 0..IP_LIMIT {
            assert!(a.allow(ip, now));
        }
        assert!(!a.allow(ip, now));
        assert!(a.allow(ip, now + WINDOW));
        assert_eq!(a.state.lock().unwrap().sources.len(), 1);
    }
    #[test]
    fn varying_sources_cannot_bypass_global_budget() {
        let a = Admission::default();
        let now = Instant::now();
        for i in 1..=GLOBAL_LIMIT {
            assert!(a.allow(IpAddr::V4(Ipv4Addr::from(i)), now));
        }
        for i in 101..10000 {
            assert!(!a.allow(IpAddr::V4(Ipv4Addr::from(i)), now));
        }
        assert_eq!(a.state.lock().unwrap().sources.len(), GLOBAL_LIMIT as usize);
    }
    #[test]
    fn rejected_source_does_not_spend_other_clients_allowance() {
        let a = Admission::default();
        let now = Instant::now();
        let source = IpAddr::V4(Ipv4Addr::LOCALHOST);
        for _ in 0..IP_LIMIT {
            assert!(a.allow(source, now));
        }
        for _ in 0..GLOBAL_LIMIT * 10 {
            assert!(!a.allow(source, now));
        }
        for i in 1..=GLOBAL_LIMIT - IP_LIMIT {
            assert!(a.allow(IpAddr::V4(Ipv4Addr::from(i)), now));
        }
        assert!(!a.allow(IpAddr::V4(Ipv4Addr::from(GLOBAL_LIMIT)), now));
        assert!(a.allow(source, now + WINDOW));
    }
    #[test]
    fn full_source_table_rejections_do_not_charge_admitted_work() {
        let a = Admission::default();
        let now = Instant::now();
        {
            let mut state = a.state.lock().unwrap();
            for i in 1..=MAX_BUCKETS as u32 {
                state.sources.insert(
                    IpAddr::V4(Ipv4Addr::from(i)),
                    Bucket {
                        start: now,
                        count: 1,
                    },
                );
            }
        }
        for _ in 0..GLOBAL_LIMIT * 2 {
            assert!(!a.allow(IpAddr::V4(Ipv4Addr::LOCALHOST), now));
        }
        assert!(a.allow(IpAddr::V4(Ipv4Addr::from(1)), now));
        assert_eq!(a.state.lock().unwrap().global.as_ref().unwrap().count, 1);
    }
    fn proxy_request(peer: Option<&str>, addresses: &[&str]) -> Request {
        let mut request = Request::builder().method("POST").uri("/api/v1/auth/login");
        if let Some(peer) = peer {
            request = request.extension(ConnectInfo(peer.parse::<SocketAddr>().unwrap()));
        }
        for address in addresses {
            request = request.header("x-opentrack-client-ip", *address);
        }
        request.body(axum::body::Body::empty()).unwrap()
    }
    #[test]
    fn forwarding_requires_an_explicit_real_trusted_peer_and_one_unicast_address() {
        let trusted: IpAddr = "192.0.2.1".parse().unwrap();
        let admission = Admission::new(vec![trusted, IpAddr::V4(Ipv4Addr::LOCALHOST)]);
        let client: IpAddr = "198.51.100.2".parse().unwrap();
        assert_eq!(
            admission
                .source(&proxy_request(Some("192.0.2.1:9000"), &["198.51.100.2"]))
                .unwrap(),
            client
        );
        assert_eq!(
            admission
                .source(&proxy_request(
                    Some("[::ffff:192.0.2.1]:9000"),
                    &["::ffff:198.51.100.2"]
                ))
                .unwrap(),
            client
        );
        assert_eq!(
            admission
                .source(&proxy_request(Some("203.0.113.1:9000"), &["198.51.100.2"]))
                .unwrap(),
            "203.0.113.1".parse::<IpAddr>().unwrap()
        );
        assert_eq!(
            admission
                .source(&proxy_request(None, &["198.51.100.2"]))
                .unwrap(),
            IpAddr::V4(Ipv4Addr::LOCALHOST)
        );
        for addresses in [
            vec![],
            vec!["198.51.100.2", "198.51.100.3"],
            vec!["198.51.100.2, 198.51.100.3"],
            vec!["198.51.100.2:9000"],
            vec!["[2001:db8::1]"],
            vec![" 198.51.100.2"],
            vec!["0.0.0.0"],
            vec!["::ffff:0.0.0.0"],
            vec!["ff02::1"],
        ] {
            assert!(
                admission
                    .source(&proxy_request(Some("192.0.2.1:9000"), &addresses))
                    .is_err(),
                "{addresses:?}"
            );
        }
        let default = Admission::default();
        assert_eq!(
            default
                .source(&proxy_request(Some("192.0.2.1:9000"), &["198.51.100.2"]))
                .unwrap(),
            trusted
        );
    }
    #[tokio::test]
    async fn independent_proxy_clients_keep_their_own_authentication_allowance() {
        use tower::ServiceExt;
        let admission = Arc::new(Admission::new(vec!["192.0.2.1".parse().unwrap()]));
        let router = axum::Router::new()
            .route(
                "/api/v1/auth/login",
                axum::routing::post(|| async { axum::http::StatusCode::NO_CONTENT }),
            )
            .layer(axum::middleware::from_fn_with_state(admission, limit_auth));
        for _ in 0..IP_LIMIT {
            assert_eq!(
                router
                    .clone()
                    .oneshot(proxy_request(Some("192.0.2.1:9000"), &["198.51.100.2"]))
                    .await
                    .unwrap()
                    .status(),
                axum::http::StatusCode::NO_CONTENT
            );
        }
        for _ in 0..GLOBAL_LIMIT * 2 {
            assert_eq!(
                router
                    .clone()
                    .oneshot(proxy_request(Some("192.0.2.1:9000"), &["198.51.100.2"]))
                    .await
                    .unwrap()
                    .status(),
                axum::http::StatusCode::TOO_MANY_REQUESTS
            );
        }
        assert_eq!(
            router
                .clone()
                .oneshot(proxy_request(Some("192.0.2.1:9000"), &["198.51.100.3"]))
                .await
                .unwrap()
                .status(),
            axum::http::StatusCode::NO_CONTENT
        );
        for _ in 0..IP_LIMIT {
            assert_eq!(
                router
                    .clone()
                    .oneshot(proxy_request(Some("203.0.113.1:9000"), &["198.51.100.4"]))
                    .await
                    .unwrap()
                    .status(),
                axum::http::StatusCode::NO_CONTENT
            );
        }
        assert_eq!(
            router
                .oneshot(proxy_request(Some("203.0.113.1:9000"), &["198.51.100.5"]))
                .await
                .unwrap()
                .status(),
            axum::http::StatusCode::TOO_MANY_REQUESTS
        );
    }
}
