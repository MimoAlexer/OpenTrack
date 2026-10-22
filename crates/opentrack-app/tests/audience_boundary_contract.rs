//! Machine/control credentials cannot become user sessions through either ingress.
#[allow(dead_code)]
mod support;
use opentrack_domain::{Error, SessionAudience};
use serde_json::json;
use sha2::{Digest, Sha256};
use support::Harness;
use uuid::Uuid;

#[tokio::test]
async fn internal_audience_is_rejected_by_schema_and_user_authentication() {
    let h = Harness::new().await;
    let copy = h.clone();
    let result = tokio::spawn(async move { audience_boundary(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn audience_boundary(h: &Harness) {
    let (session, web_token) = h
        .app
        .setup(serde_json::from_value(json!({
            "setup_token": h.setup_token, "email": "owner@example.test",
            "display_name": "Example Owner", "password": "Synthetic contract passphrase 912!",
            "workspace_name": "Audience contracts", "team_name": "Engineering", "team_key": "AUD"
        })).unwrap())
        .await
        .unwrap();
    let user = h.app.authenticate(&web_token).await.unwrap();
    assert_eq!(user.user.id, session.user.id);
    assert_eq!(user.audience, SessionAudience::Web);

    let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let hash = format!("{:x}", Sha256::digest(token.as_bytes()));
    let insert = || {
        sqlx::query("INSERT INTO sessions(token_hash,user_id,csrf_token,auth_time,expires_at,audience) VALUES($1,$2,'',clock_timestamp(),clock_timestamp()+interval '1 hour','internal')")
            .bind(&hash)
            .bind(session.user.id)
    };
    let error = insert().execute(&h.owner).await.unwrap_err();
    let database = error.as_database_error().unwrap();
    assert_eq!(database.code().as_deref(), Some("23514"));
    assert_eq!(database.constraint(), Some("session_audience_shape"));
    assert_eq!(SessionAudience::parse("internal"), None);
    assert!(serde_json::from_value::<SessionAudience>(json!("internal")).is_err());

    // Only this fixture's isolated schema is altered. A future machine-session
    // table/constraint expansion must not turn an otherwise valid token into a user.
    sqlx::query("ALTER TABLE sessions DROP CONSTRAINT session_audience_shape")
        .execute(&h.owner)
        .await
        .unwrap();
    insert().execute(&h.owner).await.unwrap();
    let before: chrono::DateTime<chrono::Utc> =
        sqlx::query_scalar("SELECT last_seen_at FROM sessions WHERE token_hash=$1")
            .bind(&hash)
            .fetch_one(&h.owner)
            .await
            .unwrap();
    for result in [
        h.app.authenticate(&token).await,
        h.app.authenticate_native(&token).await,
    ] {
        assert!(matches!(result, Err(Error::Unauthorized)));
    }
    let after: chrono::DateTime<chrono::Utc> =
        sqlx::query_scalar("SELECT last_seen_at FROM sessions WHERE token_hash=$1")
            .bind(&hash)
            .fetch_one(&h.owner)
            .await
            .unwrap();
    assert_eq!(before, after, "Rejected audiences cannot refresh a session");
    assert!(h.app.authenticate(&web_token).await.is_ok());
}
