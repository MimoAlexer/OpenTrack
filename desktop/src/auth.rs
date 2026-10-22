//! Ephemeral loopback callback and browser PKCE handoff. Secrets never cross IPC.
use crate::{
    credentials::{CredentialRef, valid_token},
    links::ServerOrigin,
    protocol::{self, Method, NativeError},
    state::{Pending, PendingInfo, Runtime, event},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::watch,
};
use uuid::Uuid;
use zeroize::Zeroizing;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BeginRequest {
    pub expected_generation: String,
    pub device_name: String,
}
#[derive(Serialize)]
pub struct BeginReply {
    pub generation: String,
    pub request_id: Uuid,
    pub confirmation_code: String,
    pub expires_at: String,
}
#[derive(Deserialize)]
struct StartReceipt {
    id: Uuid,
    authorization_path: String,
    confirmation_code: String,
    expires_at: DateTime<Utc>,
}
#[derive(Deserialize, Serialize)]
struct SafeUser {
    id: Uuid,
    email: String,
    display_name: String,
}
#[derive(Deserialize, Serialize)]
struct SafeWorkspace {
    id: Uuid,
    name: String,
    slug: String,
    role: String,
}
#[derive(Deserialize, Serialize)]
struct SafeSession {
    user: SafeUser,
    csrf_token: String,
    workspaces: Vec<SafeWorkspace>,
    audience: String,
}
#[derive(Deserialize)]
struct Grant {
    token_type: String,
    access_token: String,
    session_id: Uuid,
    expires_at: DateTime<Utc>,
    session: SafeSession,
}
impl Runtime {
    pub async fn begin(self: &Arc<Self>, request: BeginRequest) -> Result<BeginReply, NativeError> {
        let generation = protocol::generation(&request.expected_generation)?;
        let device = request.device_name.trim();
        if device.is_empty() || device.chars().count() > 80 || device.chars().any(char::is_control)
        {
            return Err(NativeError::invalid());
        }
        let handoff_permit = self
            .handoff_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| {
                NativeError::new(
                    "authentication_busy",
                    "Previous sign-in requests are finishing. Try again shortly.",
                    true,
                )
            })?;
        let operation = Uuid::new_v4();
        let (cancel, mut cancelled) = watch::channel(false);
        let server = {
            let mut inner = self.inner.lock().await;
            inner.check(generation)?;
            if inner.pending.is_some() {
                return Err(NativeError::new(
                    "authentication_pending",
                    "Finish or cancel the current sign-in first.",
                    false,
                ));
            }
            let server = inner.selected()?;
            inner.pending = Some(Pending {
                operation,
                info: None,
                cancel,
                finishing: false,
            });
            server
        };
        let result=async {
            let origin=ServerOrigin::parse(&server.origin).map_err(|_|NativeError::invalid())?;
            let listener=TcpListener::bind((std::net::Ipv4Addr::LOCALHOST,0)).await.map_err(|_|NativeError::new("callback_unavailable","The local sign-in listener could not start.",true))?;
            let address=listener.local_addr().map_err(|_|NativeError::invalid())?;
            let callback=format!("http://{address}/opentrack/native-callback");
            let mut random=[0u8;32];getrandom::fill(&mut random).map_err(|_|NativeError::invalid())?;
            let verifier=Zeroizing::new(URL_SAFE_NO_PAD.encode(random));
            getrandom::fill(&mut random).map_err(|_|NativeError::invalid())?;
            let state=Zeroizing::new(random.iter().map(|b|format!("{b:02x}")).collect::<String>());
            let challenge=URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
            let token=self.token(&server).await?;
            if server.credential.is_some()&&token.is_none(){return Err(NativeError::new("session_expired","Sign out locally before starting a new sign-in.",false));}
            self.inner.lock().await.check(generation)?;
            tokio::select!{biased; _=cancelled.changed()=>return Err(NativeError::stale()), result=self.transport.capabilities(&origin)=>result?};
            let body=serde_json::to_vec(&serde_json::json!({"code_challenge":challenge,"callback_uri":callback,"state":state.as_str(),"device_name":device})).map_err(|_|NativeError::invalid())?;
            let headers = BTreeMap::new();
            let response=tokio::select!{biased; _=cancelled.changed()=>return Err(NativeError::stale()), response=self.transport.send(&origin,Method::Post,"/api/v1/auth/native/requests",None,&headers,Some(body),token.as_deref().map(|t|t.as_str()))=>response?};
            if response.status!=201{return Err(auth_error(response.status));}
            let receipt:StartReceipt=response.json()?;
            if receipt.authorization_path!=format!("/auth/native/approve?request_id={}",receipt.id)
                || receipt.confirmation_code.len()!=8 || !receipt.confirmation_code.bytes().all(|b|b.is_ascii_uppercase()||b.is_ascii_digit())
                || receipt.expires_at<=Utc::now() || receipt.expires_at>Utc::now()+chrono::Duration::minutes(11){return Err(NativeError::server());}
            let expires_at=receipt.expires_at.to_rfc3339();
            {
                let mut inner=self.inner.lock().await;inner.check(generation)?;
                let pending=inner.pending.as_mut().filter(|p|p.operation==operation).ok_or_else(NativeError::stale)?;
                pending.info=Some(PendingInfo{request_id:receipt.id,confirmation_code:receipt.confirmation_code.clone(),expires_at:expires_at.clone()});
                (self.browser)(&format!("{}{}",origin.as_str(),receipt.authorization_path))?;
                (self.emit)(event(&inner.state(),"pending",Some(receipt.id),None,None,None));
            }
            let runtime=self.clone();let request_id=receipt.id;
            let seconds=(receipt.expires_at-Utc::now()).to_std().unwrap_or(Duration::ZERO).min(Duration::from_secs(600));
            tokio::spawn(async move {
                let _handoff_permit = handoff_permit;
                let mut exchange_started = false;
                let completion=async {
                    let code=wait_callback(listener,request_id,state,cancelled.clone(),seconds).await?;
                    {
                        let mut inner = runtime.inner.lock().await;
                        inner.check(generation)?;
                        let pending = inner.pending.as_mut().filter(|pending| pending.operation == operation).ok_or_else(NativeError::stale)?;
                        pending.finishing = true;
                    }
                    let body=serde_json::to_vec(&serde_json::json!({"request_id":request_id,"code":code.as_str(),"code_verifier":verifier.as_str()})).map_err(|_|NativeError::invalid())?;
                    let headers = BTreeMap::new();
                    exchange_started = true;
                    let response = match runtime.transport.send(&origin,Method::Post,"/api/v1/auth/native/exchange",None,&headers,Some(body),None).await {
                        Ok(response) => response,
                        Err(error) => return Err(error)
                    };
                    if response.status!=200{return Err(auth_error(response.status));}
                    let grant:Grant=response.json()?;
                    let token=Zeroizing::new(grant.access_token);
                    if grant.token_type!="Bearer"||!valid_token(&token)||grant.expires_at<=Utc::now()||grant.session.audience!="desktop"||!grant.session.csrf_token.is_empty(){return Err(NativeError::server());}
                    let reference=CredentialRef{user_id:grant.session.user.id,session_id:grant.session_id};
                    let session=serde_json::to_value(grant.session).map_err(|_|NativeError::server())?;
                    runtime.commit_grant(generation,operation,&server,reference,token,session).await
                }.await;
                if let Err(error)=completion{
                    if exchange_started { runtime.uncertain_exchange(generation, &server).await; }
                    runtime.auth_failed(generation,operation,error).await;
                }
            });
            Ok(BeginReply{generation:generation.to_string(),request_id:receipt.id,confirmation_code:receipt.confirmation_code,expires_at})
        }.await;
        if let Err(error) = &result {
            self.auth_failed(generation, operation, error.clone()).await;
        }
        result
    }
}
fn auth_error(status: u16) -> NativeError {
    match status {
        401 => NativeError::new(
            "session_expired",
            "The desktop session ended. Sign out locally and sign in again.",
            false,
        ),
        429 => NativeError::new(
            "rate_limited",
            "Too many sign-in attempts. Wait before trying again.",
            true,
        ),
        500..=599 => NativeError::new(
            "server_unavailable",
            "The server could not complete sign-in. Try again shortly.",
            true,
        ),
        _ => NativeError::new(
            "native_auth_invalid",
            "This sign-in request is no longer valid. Start again.",
            false,
        ),
    }
}
async fn wait_callback(
    listener: TcpListener,
    request_id: Uuid,
    state: Zeroizing<String>,
    mut cancel: watch::Receiver<bool>,
    duration: Duration,
) -> Result<Zeroizing<String>, NativeError> {
    let deadline = tokio::time::Instant::now() + duration;
    loop {
        let accepted = tokio::select! {biased; _=cancel.changed()=>return Err(NativeError::stale()), _=tokio::time::sleep_until(deadline)=>return Err(NativeError::new("authentication_expired","The browser sign-in request expired. Start again.",false)),result=listener.accept()=>result.map_err(|_|NativeError::network())?};
        let (mut stream, peer) = accepted;
        if !peer.ip().is_loopback() {
            continue;
        }
        let host = listener
            .local_addr()
            .map_err(|_| NativeError::invalid())?
            .to_string();
        let read = async {
            let mut bytes = Zeroizing::new(Vec::new());
            loop {
                let mut buffer = [0u8; 1024];
                let count = stream
                    .read(&mut buffer)
                    .await
                    .map_err(|_| NativeError::invalid())?;
                if count == 0 || bytes.len() + count > 8192 {
                    return Err(NativeError::invalid());
                }
                bytes.extend_from_slice(&buffer[..count]);
                if bytes.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let input = std::str::from_utf8(&bytes).map_err(|_| NativeError::invalid())?;
            parse_callback(input, &host, request_id, &state)
        };
        let code = tokio::select! {biased; _=cancel.changed()=>return Err(NativeError::stale()), result=tokio::time::timeout(Duration::from_secs(2),read)=>result.unwrap_or_else(|_|Err(NativeError::invalid()))};
        let (status, body) = if code.is_ok() {
            (
                "200 OK",
                "You can return to OpenTrack. Sign-in will finish there.",
            )
        } else {
            (
                "400 Bad Request",
                "This callback was not accepted. Return to OpenTrack to start again.",
            )
        };
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nContent-Security-Policy: default-src 'none'; frame-ancestors 'none'\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = tokio::time::timeout(
            Duration::from_secs(1),
            stream.write_all(response.as_bytes()),
        )
        .await;
        if let Ok(code) = code {
            return Ok(code);
        }
    }
}
fn parse_callback(
    input: &str,
    host: &str,
    request_id: Uuid,
    state: &str,
) -> Result<Zeroizing<String>, NativeError> {
    let mut lines = input.split("\r\n");
    let first = lines.next().ok_or_else(NativeError::invalid)?;
    let mut parts = first.split(' ');
    if parts.next() != Some("GET") {
        return Err(NativeError::invalid());
    }
    let target = parts.next().ok_or_else(NativeError::invalid)?;
    if parts.next() != Some("HTTP/1.1") || parts.next().is_some() {
        return Err(NativeError::invalid());
    }
    let mut host_seen = false;
    for line in lines {
        if line.is_empty() {
            break;
        }
        let (name, value) = line.split_once(':').ok_or_else(NativeError::invalid)?;
        if name.eq_ignore_ascii_case("host") {
            if host_seen || value.trim() != host {
                return Err(NativeError::invalid());
            }
            host_seen = true;
        }
        if name.eq_ignore_ascii_case("content-length") && value.trim() != "0"
            || name.eq_ignore_ascii_case("transfer-encoding")
        {
            return Err(NativeError::invalid());
        }
    }
    if !host_seen
        || !target.starts_with("/opentrack/native-callback?")
        || target.contains(['#', '\\'])
    {
        return Err(NativeError::invalid());
    }
    let query = target
        .strip_prefix("/opentrack/native-callback?")
        .ok_or_else(NativeError::invalid)?;
    if query.contains(['%', '+']) {
        return Err(NativeError::invalid());
    }
    let mut id = None;
    let mut code = None;
    let mut received_state = None;
    for (name, value) in url::form_urlencoded::parse(query.as_bytes()) {
        match name.as_ref() {
            "request_id" if id.is_none() => id = Some(value.into_owned()),
            "code" if code.is_none() => code = Some(Zeroizing::new(value.into_owned())),
            "state" if received_state.is_none() => {
                received_state = Some(Zeroizing::new(value.into_owned()))
            }
            _ => return Err(NativeError::invalid()),
        }
    }
    if id.as_deref() != Some(request_id.to_string().as_str())
        || received_state.as_deref().map(|s| s.as_str()) != Some(state)
        || code.as_ref().is_none_or(|s| !valid_token(s))
    {
        return Err(NativeError::invalid());
    }
    code.ok_or_else(NativeError::invalid)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn callback_requires_the_complete_pending_binding_and_exact_host() {
        let id = Uuid::new_v4();
        let state = "a".repeat(64);
        let code = "b".repeat(64);
        let request = format!(
            "GET /opentrack/native-callback?request_id={id}&code={code}&state={state} HTTP/1.1\r\nHost: 127.0.0.1:4567\r\n\r\n"
        );
        assert!(parse_callback(&request, "127.0.0.1:4567", id, &state).is_ok());
        for bad in [
            request.replace("state=", "state=x"),
            request.replace("&code=", "&unknown="),
            request.replace(" HTTP/1.1", &format!("&code={code} HTTP/1.1")),
            request.replace("127.0.0.1:4567", "evil.test"),
            request.replace("GET ", "POST "),
            request.replace("\r\n\r\n", "\r\nContent-Length: 12\r\n\r\n"),
        ] {
            assert!(parse_callback(&bad, "127.0.0.1:4567", id, &state).is_err());
        }
    }
}
