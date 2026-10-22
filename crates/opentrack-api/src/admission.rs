use crate::ApiError;
use axum::{
    extract::{ConnectInfo, Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
};
use opentrack_domain::Error;
use std::{
    collections::HashMap,
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
    fn allow(&self, source: IpAddr, now: Instant) -> bool {
        let Ok(mut state) = self.state.lock() else {
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
            return false;
        }
        global.count += 1;
        if state.sources.len() >= MAX_BUCKETS && !state.sources.contains_key(&source) {
            return false;
        }
        let bucket = state.sources.entry(source).or_insert(Bucket {
            start: now,
            count: 0,
        });
        if bucket.count >= IP_LIMIT {
            return false;
        }
        bucket.count += 1;
        true
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
        // Only the socket peer is trusted. Forwarded headers cannot create fresh
        // rate-limit identities. The fallback is for in-process router tests.
        let source = request
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map(|v| v.0.ip())
            .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));
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
}
