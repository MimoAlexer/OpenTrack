//! Opt-in local integration probe. Uses the real native bridge and OS vault,
//! while a browser approves and later revokes the session. It is not GUI proof.
use opentrack_desktop::{
    auth::BeginRequest,
    credentials::SystemVault,
    links::ServerOrigin,
    persistence::Persistence,
    protocol::{ApiRequest, Method, NativeError},
    state::{Expected, Runtime, SelectRequest},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::mpsc;
use uuid::Uuid;

async fn exercise(
    native: &Arc<Runtime>,
    origin: String,
    mut events: mpsc::Receiver<opentrack_desktop::state::AuthEvent>,
) -> Result<(), NativeError> {
    let selected = native
        .select(SelectRequest {
            expected_generation: native.read().await.generation,
            origin,
            development_http: true,
        })
        .await?;
    let pending = native
        .begin(BeginRequest {
            expected_generation: selected.generation,
            device_name: "Native bridge verification".into(),
        })
        .await?;
    println!(
        "{}",
        json!({"stage":"approve_in_browser","confirmation_code":pending.confirmation_code,"request_id":pending.request_id})
    );
    let authenticated = tokio::time::timeout(Duration::from_secs(240), async {
        while let Some(event) = events.recv().await {
            if event.kind == "authenticated" {
                return event.session.ok_or_else(NativeError::server);
            }
            if event.kind == "failed" || event.kind == "signed_out" {
                return Err(event.error.unwrap_or_else(NativeError::server));
            }
        }
        Err(NativeError::server())
    })
    .await
    .map_err(|_| NativeError::network())??;
    if authenticated["audience"] != "desktop" || authenticated["csrf_token"] != "" {
        return Err(NativeError::server());
    }
    let workspace = authenticated["workspaces"][0]["id"]
        .as_str()
        .ok_or_else(NativeError::server)?;
    let base = format!("/api/v1/workspaces/{workspace}");
    let boot = call(native, Method::Get, format!("{base}/bootstrap"), None).await?;
    let team = boot["teams"][0]["id"]
        .as_str()
        .ok_or_else(NativeError::server)?;
    let issue = call(
        native,
        Method::Post,
        format!("{base}/issues"),
        Some(json!({"team_id":team,"title":"Verify shared desktop transport"})),
    )
    .await?;
    println!(
        "{}",
        json!({"stage":"created_shared_issue","workspace":workspace,"issue":issue["id"],"key":issue["key"],"credential_present":native.read().await.credential_present})
    );
    // A human/browser test now revokes this named session from Account security.
    let revoked = tokio::time::timeout(Duration::from_secs(240), async {
        loop {
            let response = native
                .api(ApiRequest {
                    expected_generation: native.read().await.generation,
                    method: Method::Get,
                    path: "/api/v1/auth/session".into(),
                    query: None,
                    headers: BTreeMap::new(),
                    body: None,
                })
                .await?;
            if response.status == 401 {
                return if native.read().await.credential_present {
                    Err(NativeError::storage())
                } else {
                    Ok(())
                };
            }
            if response.status != 200 {
                return Err(NativeError::server());
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    })
    .await
    .map_err(|_| NativeError::network())?;
    revoked?;
    println!(
        "{}",
        json!({"stage":"remote_revocation_observed","credential_present":false})
    );
    Ok(())
}
async fn call(
    native: &Runtime,
    method: Method,
    path: String,
    body: Option<Value>,
) -> Result<Value, NativeError> {
    let mut headers = BTreeMap::new();
    if method == Method::Post {
        headers.insert("idempotency-key".into(), Uuid::new_v4().to_string());
    }
    let response = native
        .api(ApiRequest {
            expected_generation: native.read().await.generation,
            method,
            path,
            query: None,
            headers,
            body,
        })
        .await?;
    if !(200..300).contains(&response.status) {
        return Err(NativeError::server());
    }
    response.body.ok_or_else(NativeError::server)
}
#[tokio::main]
async fn main() {
    let origin = std::env::var("OPENTRACK_PROBE_SERVER")
        .expect("Provide the dedicated local test server origin");
    let origin = ServerOrigin::parse(&origin).expect("Valid server origin");
    assert!(
        origin.as_str().starts_with("http://127.0.0.1:"),
        "Probe only accepts a dedicated loopback test server"
    );
    let parent = PathBuf::from(
        std::env::var("OPENTRACK_PROBE_DIRECTORY")
            .expect("Provide the local probe output directory"),
    );
    assert!(parent.is_absolute() && parent.is_dir());
    let directory = parent.join(format!("handoff-probe-{}", Uuid::new_v4()));
    let (send, events) = mpsc::channel(16);
    let native = Runtime::new(
        Persistence::new(directory.clone()).unwrap(),
        Arc::new(SystemVault),
        Arc::new(move |event| {
            let _ = send.try_send(event);
        }),
        Arc::new(|url| {
            println!("{}", json!({"stage":"browser_url","url":url}));
            Ok(())
        }),
    )
    .unwrap();
    let result = exercise(&native, origin.as_str().into(), events).await;
    if native.read().await.credential_present {
        let _ = native
            .logout(Expected {
                expected_generation: native.read().await.generation,
            })
            .await;
    }
    let cleanup = native
        .forget(Expected {
            expected_generation: native.read().await.generation,
        })
        .await;
    if result.is_ok() && cleanup.is_ok() {
        let _ = std::fs::remove_dir_all(directory);
    }
    match result.and(cleanup.map(|_| ())) {
        Ok(()) => {
            println!("Native bridge probe passed; local configuration and credential removed.")
        }
        Err(error) => {
            eprintln!("Native bridge probe failed: {}", error.code);
            std::process::exit(1);
        }
    }
}
