//! One bounded HTTP client. Credentials are attached only after selecting a fixed origin.
use crate::{
    links::ServerOrigin,
    protocol::{Method, NativeError},
};
use reqwest::{Client, redirect::Policy};
use serde::Serialize;
use serde_json::Value;
use std::{collections::BTreeMap, time::Duration};
use zeroize::Zeroizing;

#[derive(Clone)]
pub struct Transport {
    client: Client,
}
pub struct Reply {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub bytes: Zeroizing<Vec<u8>>,
}
#[derive(Serialize)]
pub struct ApiReply {
    pub generation: String,
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Option<Value>,
}
impl Reply {
    pub fn json<T: serde::de::DeserializeOwned>(&self) -> Result<T, NativeError> {
        serde_json::from_slice(&self.bytes).map_err(|_| NativeError::server())
    }
    pub fn api(self, generation: u64) -> Result<ApiReply, NativeError> {
        let body = if self.status == 204 || self.status == 304 || self.bytes.is_empty() {
            None
        } else {
            Some(self.json()?)
        };
        Ok(ApiReply {
            generation: generation.to_string(),
            status: self.status,
            headers: self.headers,
            body,
        })
    }
}
impl Transport {
    pub fn new() -> Result<Self, NativeError> {
        let client = Client::builder()
            .redirect(Policy::none())
            .no_proxy()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .user_agent("OpenTrackDesktop/0.1 protocol/1")
            .build()
            .map_err(|_| NativeError::network())?;
        Ok(Self { client })
    }
    // Keep the credential visibly separate from caller-controlled HTTP fields.
    #[allow(clippy::too_many_arguments)]
    pub async fn send(
        &self,
        origin: &ServerOrigin,
        method: Method,
        path: &str,
        query: Option<&str>,
        headers: &BTreeMap<String, String>,
        body: Option<Vec<u8>>,
        token: Option<&str>,
    ) -> Result<Reply, NativeError> {
        // Paths reaching here are either checked IPC paths or fixed native-auth paths.
        if !path.starts_with("/api/v1/") || path.contains(['?', '#', '\\']) {
            return Err(NativeError::invalid());
        }
        let mut url = url::Url::parse(&format!("{}{path}", origin.as_str()))
            .map_err(|_| NativeError::invalid())?;
        url.set_query(query);
        if url.origin().ascii_serialization() != origin.as_str() {
            return Err(NativeError::invalid());
        }
        let long_finalize = method == Method::Post && attachment_finalize(path);
        let method = reqwest::Method::from_bytes(method.as_str().as_bytes())
            .map_err(|_| NativeError::invalid())?;
        let mut request = self
            .client
            .request(method, url)
            .header("Accept", "application/json");
        if long_finalize {
            request = request.timeout(Duration::from_secs(90));
        }
        for (key, value) in headers {
            request = request.header(key, value);
        }
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        if let Some(body) = body {
            request = request
                .header("Content-Type", "application/json")
                .body(body);
        }
        let mut response = request.send().await.map_err(|_| NativeError::network())?;
        let status = response.status().as_u16();
        if response.status().is_redirection() && status != 304 {
            return Err(NativeError::new(
                "redirect_denied",
                "The server tried to redirect this request. Check its configured address.",
                false,
            ));
        }
        let mut forwarded = BTreeMap::new();
        for name in [
            "etag",
            "location",
            "retry-after",
            "x-opentrack-session-invalidated",
        ] {
            if let Some(value) = response.headers().get(name).and_then(|h| h.to_str().ok()) {
                if value.len() > 2048 {
                    return Err(NativeError::server());
                }
                if name == "location"
                    && (!value.starts_with("/api/v1/")
                        || value.starts_with("//")
                        || value.contains(['\\', '#']))
                {
                    return Err(NativeError::server());
                }
                forwarded.insert(name.to_owned(), value.to_owned());
            }
        }
        // An imported legacy description can exceed the new write limit after
        // JSON escaping and its plaintext projection (up to six bytes per old
        // input byte in each copy). Keep this exception scoped to document reads.
        let response_limit = if path.contains("/documents/") || path.ends_with("/document") {
            4 * 1024 * 1024
        } else {
            1024 * 1024
        };
        if response
            .content_length()
            .is_some_and(|n| n > response_limit as u64)
        {
            return Err(NativeError::server());
        }
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .trim();
        if !matches!(status, 204 | 304)
            && !matches!(
                content_type,
                "application/json" | "application/problem+json"
            )
        {
            return Err(NativeError::server());
        }
        let mut bytes = Zeroizing::new(Vec::new());
        while let Some(chunk) = response.chunk().await.map_err(|_| NativeError::network())? {
            if bytes.len() + chunk.len() > response_limit {
                return Err(NativeError::server());
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(Reply {
            status,
            headers: forwarded,
            bytes,
        })
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn upload_attachment(
        &self,
        origin: &ServerOrigin,
        workspace: uuid::Uuid,
        attachment: uuid::Uuid,
        etag: &str,
        length: u64,
        body: reqwest::Body,
        token: &str,
    ) -> Result<Reply, NativeError> {
        let response = self
            .client
            .put(format!(
                "{}/api/v1/workspaces/{workspace}/attachments/{attachment}/content",
                origin.as_str()
            ))
            .timeout(Duration::from_secs(180))
            .header("Accept", "application/json")
            .header("Content-Type", "application/octet-stream")
            .header("Content-Length", length)
            .header("If-Match", etag)
            .bearer_auth(token)
            .body(body)
            .send()
            .await
            .map_err(|_| NativeError::network())?;
        attachment_reply(response).await
    }
    pub(crate) async fn download_attachment(
        &self,
        origin: &ServerOrigin,
        workspace: uuid::Uuid,
        attachment: uuid::Uuid,
        token: &str,
    ) -> Result<reqwest::Response, NativeError> {
        let response = self
            .client
            .get(format!(
                "{}/api/v1/workspaces/{workspace}/attachments/{attachment}/content",
                origin.as_str()
            ))
            .timeout(Duration::from_secs(180))
            .header("Accept", "application/octet-stream")
            .bearer_auth(token)
            .send()
            .await
            .map_err(|_| NativeError::network())?;
        if response.status().is_redirection() {
            return Err(NativeError::new(
                "redirect_denied",
                "The server tried to redirect this download.",
                false,
            ));
        }
        Ok(response)
    }
    pub async fn revoke(&self, origin: &ServerOrigin, token: &str) -> bool {
        self.send(
            origin,
            Method::Post,
            "/api/v1/auth/logout",
            None,
            &BTreeMap::new(),
            None,
            Some(token),
        )
        .await
        .is_ok_and(|r| r.status == 204 || r.status == 401)
    }
    pub async fn capabilities(&self, origin: &ServerOrigin) -> Result<(), NativeError> {
        let response = self
            .send(
                origin,
                Method::Get,
                "/api/v1/client-capabilities",
                None,
                &BTreeMap::new(),
                None,
                None,
            )
            .await?;
        let value: Value = response.json()?;
        if response.status != 200
            || value["api_version"] != "v1"
            || value["native_auth_version"] != 1
            || value["desktop_min_protocol"].as_u64().is_none_or(|n| n > 1)
        {
            return Err(NativeError::new(
                "incompatible_server",
                "This server does not support this desktop version.",
                false,
            ));
        }
        Ok(())
    }
}

fn attachment_finalize(path: &str) -> bool {
    let parts: Vec<_> = path.split('/').collect();
    matches!(parts.as_slice(), ["", "api", "v1", "workspaces", workspace, "attachments", attachment, "finalize"]
        if uuid::Uuid::parse_str(workspace).is_ok_and(|id|id.to_string()==*workspace)
        && uuid::Uuid::parse_str(attachment).is_ok_and(|id|id.to_string()==*attachment))
}
async fn attachment_reply(mut response: reqwest::Response) -> Result<Reply, NativeError> {
    let status = response.status().as_u16();
    if response.status().is_redirection() {
        return Err(NativeError::new(
            "redirect_denied",
            "The server tried to redirect this upload.",
            false,
        ));
    }
    let mime = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim();
    if !matches!(mime, "application/json" | "application/problem+json")
        || response.content_length().is_some_and(|n| n > 1024 * 1024)
    {
        return Err(NativeError::server());
    }
    let mut headers = BTreeMap::new();
    for name in ["etag", "retry-after", "x-opentrack-session-invalidated"] {
        if let Some(value) = response.headers().get(name).and_then(|v| v.to_str().ok()) {
            if value.len() > 2048 {
                return Err(NativeError::server());
            }
            headers.insert(name.to_string(), value.to_string());
        }
    }
    let mut bytes = Zeroizing::new(Vec::new());
    while let Some(chunk) = response.chunk().await.map_err(|_| NativeError::network())? {
        if bytes.len() + chunk.len() > 1024 * 1024 {
            return Err(NativeError::server());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(Reply {
        status,
        headers,
        bytes,
    })
}
