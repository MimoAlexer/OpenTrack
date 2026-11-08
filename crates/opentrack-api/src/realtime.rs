//! Bounded SSE delivery. PostgreSQL/source authorization remains in App; this
//! module owns only transport, wakeups, backpressure and connection lifetimes.
use crate::{ApiError, ApiState, Auth};
use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue},
    response::{IntoResponse, Response, Sse, sse::Event},
    routing::get,
};
use futures_util::stream;
use opentrack_app::{App, RealtimeBatch};
use opentrack_domain::{Actor, Error, RealtimeInvalidation};
use serde::Deserialize;
use std::{
    collections::HashMap,
    convert::Infallible,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, broadcast, mpsc, watch};
use uuid::Uuid;

const MAX_STREAMS: usize = 256;
const MAX_SESSION_STREAMS: usize = 3;
const MAX_EVENTS: usize = 256;
const MAX_BYTES: usize = 1024 * 1024;
const PAGE_SIZE: u32 = 100;
const SEND_TIMEOUT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_secs(2);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Copy, Default)]
pub struct RealtimeStats {
    pub active_streams: usize,
    pub queued_events: usize,
    pub queued_bytes: usize,
    pub overflowed_streams: usize,
    /// High-water marks for one client, not sums across unrelated clients.
    pub max_queued_events: usize,
    pub max_queued_bytes: usize,
}

#[derive(Default)]
struct Metrics {
    active: AtomicUsize,
    events: AtomicUsize,
    bytes: AtomicUsize,
    overflowed: AtomicUsize,
    max_events: AtomicUsize,
    max_bytes: AtomicUsize,
}

#[derive(Default)]
struct LocalAdmission {
    active: usize,
    sessions: HashMap<String, usize>,
}

/// One per serving node. The listener is lazy and unique; notifications carry
/// no content and only wake durable replay. Polling remains authoritative.
pub struct RealtimeNode {
    app: App,
    listen: bool,
    listener_started: AtomicBool,
    wake: broadcast::Sender<()>,
    shutdown: watch::Sender<bool>,
    admission: Mutex<LocalAdmission>,
    metrics: Arc<Metrics>,
}

impl RealtimeNode {
    pub fn new(app: App) -> Self {
        Self::configured(app, true)
    }

    /// The same durable delivery path with notifications unavailable. This is
    /// also a deterministic seam for safety-poll and listener-loss contracts.
    pub fn polling_only(app: App) -> Self {
        Self::configured(app, false)
    }

    fn configured(app: App, listen: bool) -> Self {
        let (wake, _) = broadcast::channel(64);
        let (shutdown, _) = watch::channel(false);
        Self {
            app,
            listen,
            listener_started: AtomicBool::new(false),
            wake,
            shutdown,
            admission: Mutex::new(LocalAdmission::default()),
            metrics: Arc::new(Metrics::default()),
        }
    }

    pub fn stats(&self) -> RealtimeStats {
        let m = &self.metrics;
        RealtimeStats {
            active_streams: m.active.load(Ordering::Relaxed),
            queued_events: m.events.load(Ordering::Relaxed),
            queued_bytes: m.bytes.load(Ordering::Relaxed),
            overflowed_streams: m.overflowed.load(Ordering::Relaxed),
            max_queued_events: m.max_events.load(Ordering::Relaxed),
            max_queued_bytes: m.max_bytes.load(Ordering::Relaxed),
        }
    }

    /// Stop the listener and close streams with a recoverable reset. No cleanup
    /// task is spawned from Drop; producers release their own durable leases.
    pub fn shutdown(&self) {
        self.shutdown.send_replace(true);
    }

    fn admit(self: &Arc<Self>, actor: &Actor) -> Result<Arc<LocalGuard>, Error> {
        if *self.shutdown.borrow() {
            return Err(Error::Unavailable);
        }
        let mut admission = self.admission.lock().unwrap_or_else(|e| e.into_inner());
        let session_count = admission
            .sessions
            .get(&actor.token_hash)
            .copied()
            .unwrap_or(0);
        if admission.active >= MAX_STREAMS || session_count >= MAX_SESSION_STREAMS {
            return Err(Error::RateLimited);
        }
        admission.active += 1;
        admission
            .sessions
            .insert(actor.token_hash.clone(), session_count + 1);
        self.metrics.active.fetch_add(1, Ordering::Relaxed);
        Ok(Arc::new(LocalGuard {
            node: self.clone(),
            session: actor.token_hash.clone(),
        }))
    }

    fn start_listener(&self) {
        if !self.listen
            || self.listener_started.swap(true, Ordering::AcqRel)
            || *self.shutdown.borrow()
        {
            return;
        }
        // This single task owns no Arc<Node>, so dropping the node can stop it.
        let app = self.app.clone();
        let wake = self.wake.clone();
        let mut shutdown = self.shutdown.subscribe();
        tokio::spawn(async move {
            loop {
                let listener = tokio::select! {
                    _ = stopped(&mut shutdown) => return,
                    result = app.listen_realtime() => result,
                };
                if let Ok(mut listener) = listener {
                    loop {
                        let received = tokio::select! {
                            _ = stopped(&mut shutdown) => return,
                            result = listener.recv() => result,
                        };
                        if received.is_err() {
                            break;
                        }
                        let _ = wake.send(());
                    }
                }
                // No payload, identifiers, credentials or provider error detail
                // enters logs. Lost wakeups are recovered by the safety poll.
                tracing::warn!("Realtime wake listener disconnected; durable polling continues");
                tokio::select! {
                    _ = stopped(&mut shutdown) => return,
                    _ = tokio::time::sleep(Duration::from_secs(1)) => {},
                }
            }
        });
    }
}

impl Drop for RealtimeNode {
    fn drop(&mut self) {
        self.shutdown.send_replace(true);
    }
}

struct LocalGuard {
    node: Arc<RealtimeNode>,
    session: String,
}

impl Drop for LocalGuard {
    fn drop(&mut self) {
        let mut admission = self
            .node
            .admission
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        admission.active -= 1;
        if let Some(count) = admission.sessions.get_mut(&self.session) {
            *count -= 1;
            if *count == 0 {
                admission.sessions.remove(&self.session);
            }
        }
        self.node.metrics.active.fetch_sub(1, Ordering::Relaxed);
    }
}

struct Control {
    terminal: watch::Sender<Option<&'static str>>,
    overflowed: AtomicBool,
    metrics: Arc<Metrics>,
}

impl Control {
    fn new(metrics: Arc<Metrics>) -> Self {
        let (terminal, _) = watch::channel(None);
        Self {
            terminal,
            overflowed: AtomicBool::new(false),
            metrics,
        }
    }

    fn reason(&self) -> Option<&'static str> {
        *self.terminal.borrow()
    }

    fn reset(&self, reason: &'static str) {
        self.terminal.send_if_modified(|value| {
            if value.is_some() {
                false
            } else {
                *value = Some(reason);
                true
            }
        });
    }

    fn overflow(&self) {
        if !self.overflowed.swap(true, Ordering::AcqRel) {
            self.metrics.overflowed.fetch_add(1, Ordering::Relaxed);
        }
        self.reset("reset_required");
    }
}

struct QueueMetrics {
    events: AtomicUsize,
    bytes: AtomicUsize,
    budget: Arc<Semaphore>,
    node: Arc<Metrics>,
}

impl QueueMetrics {
    fn new(node: Arc<Metrics>) -> Self {
        Self {
            events: AtomicUsize::new(0),
            bytes: AtomicUsize::new(0),
            budget: Arc::new(Semaphore::new(MAX_BYTES)),
            node,
        }
    }

    fn charge(self: &Arc<Self>, bytes: usize, permit: OwnedSemaphorePermit) -> QueueCharge {
        let events = self.events.fetch_add(1, Ordering::Relaxed) + 1;
        let total_bytes = self.bytes.fetch_add(bytes, Ordering::Relaxed) + bytes;
        self.node.events.fetch_add(1, Ordering::Relaxed);
        self.node.bytes.fetch_add(bytes, Ordering::Relaxed);
        self.node.max_events.fetch_max(events, Ordering::Relaxed);
        self.node
            .max_bytes
            .fetch_max(total_bytes, Ordering::Relaxed);
        QueueCharge {
            queue: self.clone(),
            bytes,
            _permit: permit,
        }
    }
}

struct QueueCharge {
    queue: Arc<QueueMetrics>,
    bytes: usize,
    _permit: OwnedSemaphorePermit,
}

impl Drop for QueueCharge {
    fn drop(&mut self) {
        self.queue.events.fetch_sub(1, Ordering::Relaxed);
        self.queue.bytes.fetch_sub(self.bytes, Ordering::Relaxed);
        self.queue.node.events.fetch_sub(1, Ordering::Relaxed);
        self.queue
            .node
            .bytes
            .fetch_sub(self.bytes, Ordering::Relaxed);
    }
}

enum FrameKind {
    Invalidation(RealtimeInvalidation),
    Checkpoint,
    Heartbeat,
}

struct QueuedFrame {
    cursor: String,
    kind: FrameKind,
    charge: QueueCharge,
}

struct BodyState {
    app: App,
    actor: Actor,
    workspace: Uuid,
    receiver: mpsc::Receiver<QueuedFrame>,
    terminal: watch::Receiver<Option<&'static str>>,
    control: Arc<Control>,
    _guard: Arc<LocalGuard>,
    done: bool,
}

async fn enqueue(
    sender: &mpsc::Sender<QueuedFrame>,
    queue: &Arc<QueueMetrics>,
    control: &Control,
    cursor: String,
    kind: FrameKind,
) -> Result<(), Error> {
    let payload_bytes = match &kind {
        FrameKind::Invalidation(event) => serde_json::to_vec(event)
            .map_err(|_| Error::Internal)?
            .len(),
        FrameKind::Checkpoint | FrameKind::Heartbeat => 2,
    };
    // Includes cursor/envelope storage; the semaphore bounds bytes independently
    // of the 256-slot channel. Queue accounting is released on dequeue or drop.
    let bytes = payload_bytes
        .saturating_add(cursor.len())
        .saturating_add(128);
    if bytes > MAX_BYTES {
        control.overflow();
        return Err(Error::Unavailable);
    }
    let send = async {
        let slot = sender.reserve().await.map_err(|_| Error::Unavailable)?;
        let permit = queue
            .budget
            .clone()
            .acquire_many_owned(bytes as u32)
            .await
            .map_err(|_| Error::Unavailable)?;
        let charge = queue.charge(bytes, permit);
        slot.send(QueuedFrame {
            cursor,
            kind,
            charge,
        });
        Ok(())
    };
    match tokio::time::timeout(SEND_TIMEOUT, send).await {
        Ok(result) => result,
        Err(_) => {
            control.overflow();
            Err(Error::Unavailable)
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StreamQuery {
    cursor: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PageQuery {
    cursor: String,
    limit: Option<u32>,
}

pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route("/workspaces/{w}/events", get(events))
        .route("/workspaces/{w}/events/page", get(page))
}

fn cursor(headers: &HeaderMap, query: Option<String>) -> Result<String, ApiError> {
    let mut values = headers.get_all("last-event-id").iter();
    let header = values
        .next()
        .map(|v| v.to_str())
        .transpose()
        .map_err(|_| Error::Invalid("Invalid replay cursor".into()))?;
    if values.next().is_some() || header.zip(query.as_deref()).is_some_and(|(a, b)| a != b) {
        return Err(Error::Invalid("Choose one unambiguous replay cursor".into()).into());
    }
    let value = header
        .map(str::to_owned)
        .or(query)
        .ok_or_else(|| Error::Invalid("A replay cursor is required".into()))?;
    validate_cursor(&value)?;
    Ok(value)
}

fn validate_cursor(cursor: &str) -> Result<(), ApiError> {
    if cursor.is_empty() || cursor.len() > 4096 || cursor.chars().any(char::is_control) {
        return Err(Error::Invalid("Invalid replay cursor".into()).into());
    }
    Ok(())
}

async fn page(
    State(state): State<ApiState>,
    Auth(actor): Auth,
    Path(workspace): Path<Uuid>,
    Query(query): Query<PageQuery>,
) -> Result<Response, ApiError> {
    validate_cursor(&query.cursor)?;
    Ok(Json(
        state
            .app
            .realtime_page(
                &actor,
                workspace,
                &query.cursor,
                query.limit.unwrap_or(PAGE_SIZE),
            )
            .await?,
    )
    .into_response())
}

async fn events(
    Extension(node): Extension<Arc<RealtimeNode>>,
    Auth(actor): Auth,
    Path(workspace): Path<Uuid>,
    Query(query): Query<StreamQuery>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let cursor = cursor(&headers, query.cursor)?;
    // Resolve malformed/expired/unauthorized replay before committing SSE headers.
    let first = node
        .app
        .realtime_replay(&actor, workspace, &cursor, PAGE_SIZE)
        .await?;
    let guard = node.admit(&actor)?;
    let lease = node.app.realtime_reserve(&actor, workspace).await?;
    node.start_listener();
    let (sender, receiver) = mpsc::channel(MAX_EVENTS);
    let queue = Arc::new(QueueMetrics::new(node.metrics.clone()));
    let control = Arc::new(Control::new(node.metrics.clone()));
    let (latest, _) = watch::channel(cursor.clone());
    let app = node.app.clone();
    let mut shutdown = node.shutdown.subscribe();
    let wake = node.wake.subscribe();
    let producer_actor = actor.clone();
    let producer_control = control.clone();
    let producer_guard = guard.clone();
    tokio::spawn(async move {
        // The monitor is polled independently of a blocked producer or client.
        // Both futures stay inside this one bounded task per admitted stream.
        let result = tokio::select! {
            _ = stopped(&mut shutdown) => Err(Error::Unavailable),
            _ = sender.closed() => Ok(()),
            result = pump(&app, &producer_actor, workspace, first, &sender, &queue, &producer_control, &latest, wake) => result,
            result = monitor(&app, &producer_actor, workspace, lease, &sender, &queue, &producer_control, latest.subscribe()) => result,
        };
        if let Err(error) = result {
            producer_control.reset(reset_reason(&error));
        }
        // This cleanup belongs to the producer, never an unbounded Drop spawn.
        // A failed cleanup is recovered by the installation lease's expiry.
        let _ = tokio::time::timeout(
            SEND_TIMEOUT,
            app.realtime_release(&producer_actor, workspace, lease),
        )
        .await;
        drop(producer_guard);
    });
    let state = BodyState {
        app: node.app.clone(),
        actor,
        workspace,
        receiver,
        terminal: control.terminal.subscribe(),
        control,
        _guard: guard,
        done: false,
    };
    let body = stream::unfold(state, |mut state| async move {
        if state.done {
            return None;
        }
        loop {
            if let Some(reason) = state.control.reason() {
                state.done = true;
                return Some((Ok::<_, Infallible>(reset_event(reason)), state));
            }
            let frame = tokio::select! {
                _ = state.terminal.changed() => continue,
                frame = state.receiver.recv() => frame,
            };
            let Some(QueuedFrame {
                cursor,
                kind,
                charge,
            }) = frame
            else {
                // A terminal reason can be installed immediately before the
                // sender closes; examine it once more before ending the stream.
                if state.control.reason().is_some() {
                    continue;
                }
                return None;
            };
            drop(charge);
            let event = match &kind {
                FrameKind::Invalidation(event) => Some(event),
                _ => None,
            };
            let authorized = tokio::select! {
                _ = state.terminal.changed() => continue,
                result = state.app.realtime_emission(&state.actor, state.workspace, &cursor, event) => result,
            };
            // Invalidation bytes are only constructed after a fresh source,
            // session and epoch decision, with no DB transaction held at yield.
            if state.control.reason().is_some() {
                continue;
            }
            let output = match authorized {
                Err(error) => {
                    state.control.reset(reset_reason(&error));
                    continue;
                }
                Ok(false) => Event::default().event("checkpoint").id(cursor).data("{}"),
                Ok(true) => match kind {
                    FrameKind::Invalidation(event) => match Event::default()
                        .event("invalidation")
                        .id(cursor)
                        .json_data(event)
                    {
                        Ok(event) => event,
                        Err(_) => {
                            state.control.reset("reset_required");
                            continue;
                        }
                    },
                    FrameKind::Checkpoint => {
                        Event::default().event("checkpoint").id(cursor).data("{}")
                    }
                    FrameKind::Heartbeat => Event::default().comment("heartbeat"),
                },
            };
            return Some((Ok::<_, Infallible>(output), state));
        }
    });
    let mut response = Sse::new(body).into_response();
    response
        .headers_mut()
        .insert("x-accel-buffering", HeaderValue::from_static("no"));
    Ok(response)
}

#[allow(clippy::too_many_arguments)]
async fn pump(
    app: &App,
    actor: &Actor,
    workspace: Uuid,
    mut batch: RealtimeBatch,
    sender: &mpsc::Sender<QueuedFrame>,
    queue: &Arc<QueueMetrics>,
    control: &Control,
    latest: &watch::Sender<String>,
    mut wake: broadcast::Receiver<()>,
) -> Result<(), Error> {
    let mut poll =
        tokio::time::interval_at(tokio::time::Instant::now() + POLL_INTERVAL, POLL_INTERVAL);
    poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        for frame in batch.frames {
            enqueue(
                sender,
                queue,
                control,
                frame.cursor,
                FrameKind::Invalidation(frame.event),
            )
            .await?;
        }
        if batch.advanced {
            // This also covers hidden rows after the final readable event.
            enqueue(
                sender,
                queue,
                control,
                batch.cursor.clone(),
                FrameKind::Checkpoint,
            )
            .await?;
        }
        let cursor = batch.cursor;
        latest.send_replace(cursor.clone());
        if !batch.has_more {
            tokio::select! {
                _ = poll.tick() => {},
                _ = wake.recv() => {},
            }
        }
        batch = app
            .realtime_replay(actor, workspace, &cursor, PAGE_SIZE)
            .await?;
    }
}

#[allow(clippy::too_many_arguments)]
async fn monitor(
    app: &App,
    actor: &Actor,
    workspace: Uuid,
    lease: Uuid,
    sender: &mpsc::Sender<QueuedFrame>,
    queue: &Arc<QueueMetrics>,
    control: &Control,
    latest: watch::Receiver<String>,
) -> Result<(), Error> {
    let mut heartbeat = tokio::time::interval_at(
        tokio::time::Instant::now() + HEARTBEAT_INTERVAL,
        HEARTBEAT_INTERVAL,
    );
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        heartbeat.tick().await;
        app.realtime_renew(actor, workspace, lease).await?;
        let cursor = latest.borrow().clone();
        app.realtime_emission(actor, workspace, &cursor, None)
            .await?;
        enqueue(sender, queue, control, cursor, FrameKind::Heartbeat).await?;
    }
}

fn reset_reason(error: &Error) -> &'static str {
    match error {
        Error::RealtimeReset(reason) => reason.as_str(),
        Error::Unauthorized | Error::Forbidden | Error::NotFound => "access_changed",
        _ => "reset_required",
    }
}

fn reset_event(reason: &'static str) -> Event {
    Event::default()
        .event("reset_required")
        .data(serde_json::json!({"reason":reason}).to_string())
}

async fn stopped(shutdown: &mut watch::Receiver<bool>) {
    loop {
        if *shutdown.borrow() {
            return;
        }
        if shutdown.changed().await.is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Control, FrameKind, MAX_BYTES, Metrics, QueueMetrics, cursor, enqueue};
    use axum::http::{HeaderMap, HeaderValue};
    use std::sync::{Arc, atomic::Ordering};
    use tokio::sync::mpsc;

    #[test]
    fn duplicate_or_conflicting_cursor_sources_are_rejected() {
        let mut headers = HeaderMap::new();
        headers.append("last-event-id", HeaderValue::from_static("first"));
        assert_eq!(cursor(&headers, Some("first".into())).unwrap(), "first");
        assert!(cursor(&headers, Some("second".into())).is_err());
        headers.append("last-event-id", HeaderValue::from_static("first"));
        assert!(cursor(&headers, None).is_err());
        assert!(cursor(&HeaderMap::new(), Some("\n".into())).is_err());
    }

    #[tokio::test]
    async fn dropped_queued_frames_release_every_byte_and_event_charge() {
        let metrics = Arc::new(Metrics::default());
        let queue = Arc::new(QueueMetrics::new(metrics.clone()));
        let control = Control::new(metrics.clone());
        let (sender, mut receiver) = mpsc::channel(256);
        for _ in 0..2 {
            enqueue(
                &sender,
                &queue,
                &control,
                "opaque".into(),
                FrameKind::Checkpoint,
            )
            .await
            .unwrap();
        }
        assert_eq!(metrics.events.load(Ordering::Relaxed), 2);
        assert_eq!(metrics.max_events.load(Ordering::Relaxed), 2);
        drop(receiver.recv().await.unwrap());
        assert_eq!(metrics.events.load(Ordering::Relaxed), 1);
        drop(receiver);
        assert_eq!(metrics.events.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.bytes.load(Ordering::Relaxed), 0);
        assert_eq!(queue.budget.available_permits(), MAX_BYTES);
    }

    #[tokio::test]
    async fn oversized_frame_resets_once_without_charging_or_queuing_payload() {
        let metrics = Arc::new(Metrics::default());
        let queue = Arc::new(QueueMetrics::new(metrics.clone()));
        let control = Control::new(metrics.clone());
        let (sender, mut receiver) = mpsc::channel(256);
        for _ in 0..2 {
            assert!(
                enqueue(
                    &sender,
                    &queue,
                    &control,
                    "x".repeat(MAX_BYTES),
                    FrameKind::Checkpoint,
                )
                .await
                .is_err()
            );
        }
        assert_eq!(control.reason(), Some("reset_required"));
        assert_eq!(metrics.overflowed.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.events.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.bytes.load(Ordering::Relaxed), 0);
        assert!(receiver.try_recv().is_err());
    }
}
