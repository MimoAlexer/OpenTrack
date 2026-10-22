//! Server/account scope, cancellation and credential commits share one native owner.
use crate::{
    credentials::{CredentialRef, Vault},
    links::{BrowserAction, ServerOrigin},
    persistence::{Configuration, Persistence, SavedServer},
    protocol::{self, ApiRequest, NativeError},
    transport::{ApiReply, Transport},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{sync::Arc, time::Duration};
use tokio::sync::{Mutex, Semaphore, watch};
use uuid::Uuid;
use zeroize::Zeroizing;

#[derive(Clone, Serialize)]
pub struct Selected {
    pub origin: String,
    pub development_http: bool,
}
#[derive(Clone, Serialize)]
pub struct PendingInfo {
    pub request_id: Uuid,
    pub confirmation_code: String,
    pub expires_at: String,
}
#[derive(Clone, Serialize)]
pub struct ConnectionState {
    pub generation: String,
    pub scope_generation: String,
    pub selected: Option<Selected>,
    pub credential_present: bool,
    pub pending: Option<PendingInfo>,
}
#[derive(Clone, Serialize)]
pub struct AuthEvent {
    pub generation: String,
    pub scope_generation: String,
    pub kind: &'static str,
    pub request_id: Option<Uuid>,
    pub session: Option<Value>,
    pub error: Option<NativeError>,
    pub remote_revocation_confirmed: Option<bool>,
}
#[derive(Serialize)]
pub struct LogoutState {
    #[serde(flatten)]
    pub state: ConnectionState,
    pub remote_revocation_confirmed: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expected {
    pub expected_generation: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectRequest {
    pub expected_generation: String,
    pub origin: String,
    pub development_http: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserRequest {
    pub expected_generation: String,
    pub action: BrowserAction,
}
#[derive(Serialize)]
pub struct Generation {
    pub generation: String,
}

pub(crate) struct Pending {
    pub operation: Uuid,
    pub info: Option<PendingInfo>,
    pub cancel: watch::Sender<bool>,
    pub finishing: bool,
}
#[derive(Clone, Serialize)]
pub struct IssueLinkState {
    pub id: Uuid,
    pub server: String,
    pub workspace: Uuid,
    pub issue: Uuid,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClearLinkRequest {
    pub id: Uuid,
}
pub(crate) struct Inner {
    pub generation: u64,
    pub scope: u64,
    pub config: Configuration,
    pub issue_link: Option<IssueLinkState>,
    pub pending: Option<Pending>,
    pub attachments: crate::attachments::AttachmentState,
}
impl Inner {
    fn bump(&mut self, scope: bool) -> Result<(), NativeError> {
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or_else(NativeError::invalid)?;
        if scope {
            self.scope = self.scope.checked_add(1).ok_or_else(NativeError::invalid)?;
        }
        self.attachments.reset();
        Ok(())
    }
    pub(crate) fn selected(&self) -> Result<SavedServer, NativeError> {
        self.config
            .servers
            .iter()
            .find(|s| Some(&s.origin) == self.config.selected.as_ref())
            .cloned()
            .ok_or_else(|| NativeError::new("no_connection", "Choose a server first.", false))
    }
    pub(crate) fn check(&self, generation: u64) -> Result<(), NativeError> {
        if self.generation == generation {
            Ok(())
        } else {
            Err(NativeError::stale())
        }
    }
    pub(crate) fn state(&self) -> ConnectionState {
        let selected = self.selected().ok();
        ConnectionState {
            generation: self.generation.to_string(),
            scope_generation: self.scope.to_string(),
            credential_present: selected.as_ref().is_some_and(|s| s.credential.is_some()),
            selected: selected.map(|s| Selected {
                origin: s.origin,
                development_http: s.development_http,
            }),
            pending: self.pending.as_ref().and_then(|p| p.info.clone()),
        }
    }
    pub(crate) fn cancel(&mut self) {
        if let Some(pending) = self.pending.take() {
            let _ = pending.cancel.send(true);
        }
    }
}
pub type BrowserOpener = Arc<dyn Fn(&str) -> Result<(), NativeError> + Send + Sync>;
struct VaultPermit {
    _slot: tokio::sync::OwnedSemaphorePermit,
    _lane: tokio::sync::OwnedSemaphorePermit,
}
pub struct Runtime {
    pub(crate) inner: Mutex<Inner>,
    pub(crate) transport: Transport,
    pub(crate) vault: Arc<dyn Vault>,
    pub(crate) vault_lane: Arc<Semaphore>,
    vault_slots: Arc<Semaphore>,
    pub(crate) handoff_slots: Arc<Semaphore>,
    pub(crate) persistence: Persistence,
    pub(crate) emit: Arc<dyn Fn(AuthEvent) + Send + Sync>,
    pub(crate) browser: BrowserOpener,
    pub(crate) attachment_dialogs: Arc<Semaphore>,
    pub(crate) attachment_io: Arc<Semaphore>,
}
impl Runtime {
    pub fn new(
        persistence: Persistence,
        vault: Arc<dyn Vault>,
        emit: Arc<dyn Fn(AuthEvent) + Send + Sync>,
        browser: BrowserOpener,
    ) -> Result<Arc<Self>, NativeError> {
        let config = persistence.load()?;
        Ok(Arc::new(Self {
            inner: Mutex::new(Inner {
                generation: 1,
                scope: 1,
                config,
                issue_link: None,
                pending: None,
                attachments: crate::attachments::AttachmentState::default(),
            }),
            transport: Transport::new()?,
            vault,
            vault_lane: Arc::new(Semaphore::new(1)),
            vault_slots: Arc::new(Semaphore::new(32)),
            handoff_slots: Arc::new(Semaphore::new(4)),
            persistence,
            emit,
            browser,
            attachment_dialogs: Arc::new(Semaphore::new(1)),
            attachment_io: Arc::new(Semaphore::new(4)),
        }))
    }
    pub async fn issue_link(&self) -> Option<IssueLinkState> {
        self.inner.lock().await.issue_link.clone()
    }
    pub async fn accept_link(&self, input: &str) -> Option<IssueLinkState> {
        let link = crate::links::IssueLink::parse(input).ok()?;
        let link = IssueLinkState {
            id: Uuid::new_v4(),
            server: link.server.as_str().into(),
            workspace: link.workspace,
            issue: link.issue,
        };
        self.inner.lock().await.issue_link = Some(link.clone());
        Some(link)
    }
    pub async fn clear_link(&self, request: ClearLinkRequest) -> Result<(), NativeError> {
        let mut inner = self.inner.lock().await;
        if inner
            .issue_link
            .as_ref()
            .is_some_and(|link| link.id == request.id)
        {
            inner.issue_link = None;
        }
        Ok(())
    }
    pub async fn read(&self) -> ConnectionState {
        self.inner.lock().await.state()
    }
    pub async fn select(&self, request: SelectRequest) -> Result<ConnectionState, NativeError> {
        let expected = protocol::generation(&request.expected_generation)?;
        let origin = ServerOrigin::parse(&request.origin).map_err(|_| NativeError::invalid())?;
        if origin.as_str().starts_with("http:") && !request.development_http {
            return Err(NativeError::new(
                "development_consent_required",
                "Confirm the local development connection before using HTTP.",
                false,
            ));
        }
        let mut inner = self.inner.lock().await;
        inner.check(expected)?;
        if inner.config.selected.as_deref() == Some(origin.as_str()) {
            return Ok(inner.state());
        }
        let mut config = inner.config.clone();
        if !config.servers.iter().any(|s| s.origin == origin.as_str()) {
            if config.servers.len() >= 32 {
                return Err(NativeError::new(
                    "connection_limit",
                    "Forget an unused server before adding another.",
                    false,
                ));
            }
            config.servers.push(SavedServer {
                origin: origin.as_str().into(),
                development_http: request.development_http,
                credential: None,
            });
        }
        config.selected = Some(origin.as_str().into());
        self.persistence.save(&config)?;
        inner.cancel();
        inner.config = config;
        inner.bump(true)?;
        Ok(inner.state())
    }
    async fn vault_call<T: Send + 'static>(
        &self,
        call: impl FnOnce(Arc<dyn Vault>) -> Result<T, NativeError> + Send + 'static,
    ) -> Result<T, NativeError> {
        let (permit, deadline) = self.vault_permit().await?;
        let vault = self.vault.clone();
        let task = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| call(vault)))
                .unwrap_or_else(|_| Err(NativeError::storage()))
        });
        tokio::time::timeout_at(deadline, task)
            .await
            .map_err(|_| NativeError::storage())?
            .map_err(|_| NativeError::storage())?
    }
    async fn vault_permit(&self) -> Result<(VaultPermit, tokio::time::Instant), NativeError> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        let slot = self.vault_slots.clone().try_acquire_owned().map_err(|_| {
            NativeError::new(
                "credential_store_busy",
                "Too many requests are waiting for the credential store. Try again shortly.",
                true,
            )
        })?;
        // Parallel workspace reads must wait for the current OS call, not report
        // a locked vault. Admission and the shared deadline bound this queue.
        let lane = tokio::time::timeout_at(deadline, self.vault_lane.clone().acquire_owned())
            .await
            .map_err(|_| NativeError::storage())?
            .map_err(|_| NativeError::storage())?;
        Ok((
            VaultPermit {
                _slot: slot,
                _lane: lane,
            },
            deadline,
        ))
    }
    pub(crate) async fn token(
        &self,
        server: &SavedServer,
    ) -> Result<Option<Zeroizing<String>>, NativeError> {
        let Some(reference) = server.credential.clone() else {
            return Ok(None);
        };
        let origin = server.origin.clone();
        self.vault_call(move |vault| vault.get(&origin, &reference))
            .await
    }
    async fn delete(&self, server: &SavedServer) -> Result<(), NativeError> {
        let Some(reference) = server.credential.clone() else {
            return Ok(());
        };
        let origin = server.origin.clone();
        self.vault_call(move |vault| vault.delete(&origin, &reference))
            .await
    }
    pub async fn cancel_auth(&self, request: Expected) -> Result<ConnectionState, NativeError> {
        let mut inner = self.inner.lock().await;
        inner.check(protocol::generation(&request.expected_generation)?)?;
        if inner
            .pending
            .as_ref()
            .is_some_and(|pending| pending.finishing)
        {
            return Err(NativeError::new(
                "authentication_finishing",
                "Sign-in is finishing. Wait for the result before cancelling.",
                true,
            ));
        }
        let id = inner
            .pending
            .as_ref()
            .and_then(|p| p.info.as_ref())
            .map(|p| p.request_id);
        inner.cancel();
        inner.bump(false)?;
        let state = inner.state();
        (self.emit)(event(&state, "cancelled", id, None, None, None));
        Ok(state)
    }
    pub async fn forget(&self, request: Expected) -> Result<ConnectionState, NativeError> {
        let expected = protocol::generation(&request.expected_generation)?;
        // Serialize destructive local credential changes. No network I/O under this lock.
        let mut inner = self.inner.lock().await;
        inner.check(expected)?;
        let server = inner.selected()?;
        self.delete(&server).await?;
        let mut config = inner.config.clone();
        config.servers.retain(|s| s.origin != server.origin);
        config.selected = None;
        self.persistence.save(&config)?;
        inner.cancel();
        inner.config = config;
        inner.bump(true)?;
        Ok(inner.state())
    }
    pub async fn logout(&self, request: Expected) -> Result<LogoutState, NativeError> {
        let expected = protocol::generation(&request.expected_generation)?;
        let server = {
            let inner = self.inner.lock().await;
            inner.check(expected)?;
            inner.selected()?
        };
        let token = self.token(&server).await?;
        self.inner.lock().await.check(expected)?;
        let origin = ServerOrigin::parse(&server.origin).map_err(|_| NativeError::invalid())?;
        let confirmed = if let Some(token) = &token {
            self.transport.revoke(&origin, token).await
        } else {
            false
        };
        let state = self
            .clear_credential(expected, &server, None, Some(confirmed))
            .await?;
        Ok(LogoutState {
            state,
            remote_revocation_confirmed: confirmed,
        })
    }
    pub(crate) async fn clear_credential(
        &self,
        expected: u64,
        server: &SavedServer,
        error: Option<NativeError>,
        confirmed: Option<bool>,
    ) -> Result<ConnectionState, NativeError> {
        let mut inner = self.inner.lock().await;
        inner.check(expected)?;
        self.delete(server).await?;
        let mut config = inner.config.clone();
        if let Some(s) = config
            .servers
            .iter_mut()
            .find(|s| s.origin == server.origin)
        {
            s.credential = None;
        }
        self.persistence.save(&config)?;
        inner.cancel();
        inner.config = config;
        inner.bump(true)?;
        let state = inner.state();
        (self.emit)(event(&state, "signed_out", None, None, error, confirmed));
        Ok(state)
    }
    pub async fn browser_action(&self, request: BrowserRequest) -> Result<Generation, NativeError> {
        let inner = self.inner.lock().await;
        inner.check(protocol::generation(&request.expected_generation)?)?;
        let server = inner.selected()?;
        let origin = ServerOrigin::parse(&server.origin).map_err(|_| NativeError::invalid())?;
        (self.browser)(&origin.browser_action(request.action))?;
        Ok(Generation {
            generation: inner.generation.to_string(),
        })
    }
    pub async fn api(&self, request: ApiRequest) -> Result<ApiReply, NativeError> {
        let request = request.check()?;
        let server = {
            let inner = self.inner.lock().await;
            inner.check(request.generation)?;
            inner.selected()?
        };
        let token = self.token(&server).await?;
        let Some(token) = token else {
            if server.credential.is_some() {
                self.clear_credential(
                    request.generation,
                    &server,
                    Some(NativeError::new(
                        "session_expired",
                        "Sign in again to continue.",
                        false,
                    )),
                    None,
                )
                .await?;
            }
            return Err(NativeError::new(
                "sign_in_required",
                "Sign in to this server first.",
                false,
            ));
        };
        // Recheck after asynchronous keychain lookup, before dispatch to captured origin.
        self.inner.lock().await.check(request.generation)?;
        let origin = ServerOrigin::parse(&server.origin).map_err(|_| NativeError::invalid())?;
        let reply = self
            .transport
            .send(
                &origin,
                request.method,
                &request.path,
                request.query.as_deref(),
                &request.headers,
                request.body,
                Some(&token),
            )
            .await?;
        self.inner.lock().await.check(request.generation)?;
        let invalidated = (200..300).contains(&reply.status)
            && reply
                .headers
                .get("x-opentrack-session-invalidated")
                .is_some_and(|v| v == "true");
        if reply.status == 401 || invalidated {
            let error = if invalidated {
                NativeError::new(
                    "access_changed",
                    "Your access changed. Sign in again to continue.",
                    false,
                )
            } else {
                NativeError::new(
                    "session_expired",
                    "Your session ended. Sign in again to continue.",
                    false,
                )
            };
            self.clear_credential(request.generation, &server, Some(error), None)
                .await?;
        }
        reply.api(request.generation)
    }
    pub(crate) async fn auth_failed(&self, generation: u64, operation: Uuid, error: NativeError) {
        let mut inner = self.inner.lock().await;
        if inner.generation != generation
            || inner
                .pending
                .as_ref()
                .is_none_or(|p| p.operation != operation)
        {
            return;
        }
        let request_id = inner
            .pending
            .as_ref()
            .and_then(|p| p.info.as_ref())
            .map(|p| p.request_id);
        inner.cancel();
        let state = inner.state();
        (self.emit)(event(&state, "failed", request_id, None, Some(error), None));
    }
    pub(crate) async fn uncertain_exchange(&self, _generation: u64, server: &SavedServer) {
        if server.credential.is_none() {
            return;
        }
        // The server may already have rotated this credential. It must not remain
        // usable through this client's selected connection after an ambiguous result.
        let deleted = self.delete(server).await.is_ok();
        let mut inner = self.inner.lock().await;
        let mut config = inner.config.clone();
        let Some(saved) = config
            .servers
            .iter_mut()
            .find(|s| s.origin == server.origin && s.credential == server.credential)
        else {
            return;
        };
        saved.credential = None;
        let saved = self.persistence.save(&config).is_ok();
        inner.config = config;
        if inner.config.selected.as_deref() == Some(&server.origin) {
            inner.cancel();
            let _ = inner.bump(true);
            let message = if deleted && saved {
                "Sign-in could not be confirmed. Sign in again and review active sessions."
            } else {
                "Sign-in could not be confirmed and local cleanup is incomplete. Unlock the credential store, then sign in and review active sessions."
            };
            (self.emit)(event(
                &inner.state(),
                "signed_out",
                None,
                None,
                Some(NativeError::new(
                    "reauthentication_uncertain",
                    message,
                    true,
                )),
                Some(false),
            ));
        }
    }
    pub(crate) async fn commit_grant(
        &self,
        generation: u64,
        operation: Uuid,
        server: &SavedServer,
        reference: CredentialRef,
        token: Zeroizing<String>,
        session: Value,
    ) -> Result<(), NativeError> {
        let (permit, deadline) = match self.vault_permit().await {
            Ok(permit) => permit,
            Err(error) => {
                let origin =
                    ServerOrigin::parse(&server.origin).map_err(|_| NativeError::invalid())?;
                self.transport.revoke(&origin, &token).await;
                return Err(error);
            }
        };
        let vault = self.vault.clone();
        let origin = server.origin.clone();
        let credential = reference.clone();
        let secret = token.clone();
        let mut task = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                vault.set(&origin, &credential, &secret)
            }))
            .unwrap_or_else(|_| Err(NativeError::storage()))
        });
        let mut cleanup_scheduled = false;
        let stored = match tokio::time::timeout_at(deadline, &mut task).await {
            Ok(joined) => joined.unwrap_or_else(|_| Err(NativeError::storage())),
            Err(_) => {
                cleanup_scheduled = true;
                // The one blocking lane remains held until the OS call returns. Revoke now,
                // then remove any late write; never start another blocked storage operation.
                let vault = self.vault.clone();
                let lane = self.vault_lane.clone();
                let origin = server.origin.clone();
                let credential = reference.clone();
                tokio::spawn(async move {
                    let _ = task.await;
                    if let Ok(permit) = lane.acquire_owned().await {
                        let _ = tokio::task::spawn_blocking(move || {
                            let _permit = permit;
                            vault.delete(&origin, &credential)
                        })
                        .await;
                    }
                });
                Err(NativeError::storage())
            }
        };
        if let Err(error) = stored {
            let origin = ServerOrigin::parse(&server.origin).map_err(|_| NativeError::invalid())?;
            self.transport.revoke(&origin, &token).await;
            if !cleanup_scheduled {
                let origin = server.origin.clone();
                let credential = reference.clone();
                let _ = self
                    .vault_call(move |vault| vault.delete(&origin, &credential))
                    .await;
            }
            return Err(error);
        }
        let result = {
            let mut inner = self.inner.lock().await;
            if inner.generation != generation
                || inner
                    .pending
                    .as_ref()
                    .is_none_or(|p| p.operation != operation)
            {
                Err(NativeError::stale())
            } else {
                let mut config = inner.config.clone();
                let selected = config
                    .servers
                    .iter_mut()
                    .find(|s| s.origin == server.origin)
                    .ok_or_else(NativeError::stale)?;
                let scope_changed = selected
                    .credential
                    .as_ref()
                    .is_none_or(|r| r.user_id != reference.user_id);
                selected.credential = Some(reference.clone());
                match self.persistence.save(&config) {
                    Err(error) => Err(error),
                    Ok(()) => {
                        inner.config = config;
                        let id = inner
                            .pending
                            .as_ref()
                            .and_then(|p| p.info.as_ref())
                            .map(|p| p.request_id);
                        inner.cancel();
                        inner.bump(scope_changed)?;
                        let state = inner.state();
                        (self.emit)(event(
                            &state,
                            "authenticated",
                            id,
                            Some(session),
                            None,
                            None,
                        ));
                        Ok(())
                    }
                }
            }
        };
        if result.is_err() {
            let origin = ServerOrigin::parse(&server.origin).map_err(|_| NativeError::invalid())?;
            self.transport.revoke(&origin, &token).await;
            let origin = server.origin.clone();
            let _ = self
                .vault_call(move |vault| vault.delete(&origin, &reference))
                .await;
        } else if let Some(old) = &server.credential {
            // The server rotated this row; remove its obsolete local reference as cleanup.
            let origin = server.origin.clone();
            let old = old.clone();
            let _ = self
                .vault_call(move |vault| vault.delete(&origin, &old))
                .await;
        }
        result
    }
}
pub(crate) fn event(
    state: &ConnectionState,
    kind: &'static str,
    request_id: Option<Uuid>,
    session: Option<Value>,
    error: Option<NativeError>,
    confirmed: Option<bool>,
) -> AuthEvent {
    AuthEvent {
        generation: state.generation.clone(),
        scope_generation: state.scope_generation.clone(),
        kind,
        request_id,
        session,
        error,
        remote_revocation_confirmed: confirmed,
    }
}
