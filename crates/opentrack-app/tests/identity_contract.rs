//! Real PostgreSQL identity lifecycle tests. All application calls use a runtime
//! role without table ownership/BYPASSRLS; fixture SQL is scoped to a fresh schema.
mod support;

use opentrack_domain::*;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use support::Harness;
use uuid::Uuid;

const PASSWORD: &str = "identity contract password 946!";

fn request<T: DeserializeOwned>(body: Value) -> T {
    serde_json::from_value(body).expect("The test request follows the public DTO")
}

fn page() -> DirectoryQuery {
    DirectoryQuery::default()
}

fn new_account(token: &str) -> AcceptInvitation {
    request(json!({"token":token,"display_name":"Invited collaborator","password":PASSWORD}))
}

fn issue(team: Uuid) -> CreateIssue {
    request(json!({"team_id":team,"title":"Auth boundary issue"}))
}

async fn member(h: &Harness, actor: &Actor, w: Uuid, user: Uuid) -> WorkspaceMember {
    h.app
        .workspace_members(actor, w, page())
        .await
        .unwrap()
        .data
        .into_iter()
        .find(|m| m.user.id == user)
        .expect("Member is present in the directory")
}

async fn invite(h: &Harness, owner: &Actor, w: Uuid, email: &str, role: &str) -> InvitationIssued {
    h.app
        .create_invitation(
            owner,
            w,
            &Uuid::new_v4().to_string(),
            request(json!({"email":email,"role":role})),
        )
        .await
        .expect("Authorized invitation creation")
}

async fn join(h: &Harness, owner: &Actor, w: Uuid, email: &str, role: &str) -> Actor {
    let issued = invite(h, owner, w, email, role).await;
    let (_, cookie) = h
        .app
        .accept_invitation(None, new_account(issued.token.as_deref().unwrap()))
        .await
        .expect("A new account accepts a usable invitation");
    h.app.authenticate(&cookie).await.unwrap()
}

async fn sign_in_again(h: &Harness, actor: &Actor) -> Actor {
    let (_, cookie) = h
        .app
        .login(request(
            json!({"email":actor.user.email,"password":PASSWORD}),
        ))
        .await
        .unwrap();
    h.app.authenticate(&cookie).await.unwrap()
}

async fn join_existing(h: &Harness, owner: &Actor, w: Uuid, actor: &Actor) -> Actor {
    let issued = invite(h, owner, w, &actor.user.email, "member").await;
    let (_, cookie) = h
        .app
        .accept_invitation(Some(actor), request(json!({"token":issued.token.unwrap()})))
        .await
        .expect("The matching signed-in account accepts its invitation");
    h.app.authenticate(&cookie).await.unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_postgres_identity_lifecycle_contracts() {
    let harness = Harness::new().await;
    let runner = harness.clone();
    let result = tokio::spawn(async move { exercise(&runner).await }).await;
    harness.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn exercise(h: &Harness) {
    let (session, cookie) = h
        .app
        .setup(request(json!({
            "setup_token":h.setup_token,"email":"owner@identity.invalid","password":PASSWORD,
            "display_name":"First owner","workspace_name":"Identity workspace",
            "team_name":"Engineering","team_key":"ENG"
        })))
        .await
        .unwrap();
    let owner = h.app.authenticate(&cookie).await.unwrap();
    let w = session.workspaces[0].id;
    let public_team = h.app.bootstrap(&owner, w).await.unwrap().teams[0].id;
    let organization = h.app.organizations(&owner).await.unwrap();
    assert_eq!(organization.len(), 1);
    assert_eq!(organization[0].role, "owner");
    let second = workspace_lifecycle(h, &owner, organization[0].id, w).await;
    invitation_lifecycle(h, &owner, w, second.id).await;
    let admin = join(h, &owner, w, "admin@identity.invalid", "admin").await;
    let contributor = join(h, &owner, w, "member@identity.invalid", "member").await;
    let viewer = join(h, &owner, w, "viewer@identity.invalid", "viewer").await;
    role_boundaries(
        h,
        &owner,
        &admin,
        &contributor,
        &viewer,
        w,
        organization[0].id,
    )
    .await;
    let private = private_team_lifecycle(h, &owner, &admin, &contributor, w).await;
    invitation_authority(h, &owner, &admin, w, private).await;
    let admin = sign_in_again(h, &admin).await;
    let contributor = sign_in_again(h, &contributor).await;
    revocation_race(h, &owner, &contributor, w, public_team).await;
    let contributor = sign_in_again(h, &contributor).await;
    deactivation(h, &owner, &contributor, w, second.id, public_team).await;
    concurrent_last_owner(h, &owner, &admin, organization[0].id).await;
    recent_authentication(h, &owner, w).await;
}

async fn workspace_lifecycle(
    h: &Harness,
    owner: &Actor,
    organization: Uuid,
    original: Uuid,
) -> WorkspaceDetail {
    let body = json!({"organization_id":organization,"name":"Second workspace","team_name":"Operations","team_key":"OPS"});
    let created = h
        .app
        .create_workspace(owner, "workspace-create", request(body.clone()))
        .await
        .unwrap();
    let replay = h
        .app
        .create_workspace(owner, "workspace-create", request(body.clone()))
        .await
        .unwrap();
    assert_eq!(created.id, replay.id);
    assert_ne!(created.id, original);
    assert_eq!(created.organization_id, organization);
    assert_eq!(created.role, "owner");
    let mut changed = body;
    changed["name"] = json!("Different");
    assert!(matches!(
        h.app
            .create_workspace(owner, "workspace-create", request(changed))
            .await,
        Err(Error::Conflict(_))
    ));
    let renamed = h
        .app
        .update_workspace(
            owner,
            created.id,
            &created.etag(),
            request(json!({"name":"Operations home"})),
        )
        .await
        .unwrap();
    assert_eq!(renamed.id, created.id);
    assert_eq!(renamed.slug, created.slug);
    assert_ne!(renamed.etag(), created.etag());
    assert!(matches!(
        h.app
            .update_workspace(
                owner,
                created.id,
                &created.etag(),
                request(json!({"name":"Stale overwrite"}))
            )
            .await,
        Err(Error::VersionConflict(_))
    ));
    assert_eq!(
        h.app.workspace(owner, created.id).await.unwrap().name,
        "Operations home"
    );
    let archived = h
        .app
        .update_workspace(
            owner,
            renamed.id,
            &renamed.etag(),
            request(json!({"archived":true})),
        )
        .await
        .unwrap();
    assert!(archived.archived_at.is_some());
    assert!(matches!(
        h.app
            .create_team(
                owner,
                archived.id,
                "archived-team",
                request(json!({"name":"Blocked","key":"BLOCK","private":false}))
            )
            .await,
        Err(Error::Conflict(_))
    ));
    let restored = h
        .app
        .update_workspace(
            owner,
            archived.id,
            &archived.etag(),
            request(json!({"archived":false})),
        )
        .await
        .unwrap();
    assert!(restored.archived_at.is_none());
    assert_eq!(h.app.session(owner).await.unwrap().workspaces.len(), 2);
    restored
}

async fn invitation_lifecycle(h: &Harness, owner: &Actor, w: Uuid, second: Uuid) {
    let body = json!({"email":"  Fresh@identity.invalid ","role":"member"});
    let issued = h
        .app
        .create_invitation(owner, w, "invite-new", request(body.clone()))
        .await
        .unwrap();
    let token = issued.token.as_deref().unwrap();
    assert!(token.starts_with(&format!("{w}.")));
    assert_eq!(token.split('.').nth(1).unwrap().len(), 64);
    assert_eq!(issued.invitation.email, "fresh@identity.invalid");
    let preview = h.app.invitation_preview(token).await.unwrap();
    assert_eq!(preview.workspace_id, w);
    assert_eq!(preview.email, issued.invitation.email);
    assert_eq!(preview.role, "member");
    let lifetime = issued.invitation.expires_at - chrono::Utc::now();
    assert!(lifetime.num_hours() >= 47 && lifetime.num_hours() <= 48);
    let replay = h
        .app
        .create_invitation(owner, w, "invite-new", request(body))
        .await
        .unwrap();
    assert_eq!(replay.invitation.id, issued.invitation.id);
    assert!(
        replay.token.is_none(),
        "Idempotency replay must never reproduce a secret"
    );
    let mut tx = h.context(owner.user.id, w).await;
    let persisted: String =
        sqlx::query_scalar("SELECT row_to_json(i)::text FROM invitations i WHERE id=$1")
            .bind(issued.invitation.id)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    let persisted_retry: Vec<Value> = sqlx::query_scalar("SELECT response_body FROM idempotency_records WHERE workspace_id=$1 AND response_body IS NOT NULL").bind(w).fetch_all(&mut *tx).await.unwrap();
    assert!(!persisted.contains(token));
    assert!(
        !persisted_retry
            .iter()
            .any(|v| v.to_string().contains(token))
    );
    tx.commit().await.unwrap();
    let (_, cookie) = h
        .app
        .accept_invitation(None, new_account(token))
        .await
        .unwrap();
    let fresh = h.app.authenticate(&cookie).await.unwrap();
    assert_eq!(fresh.user.email, "fresh@identity.invalid");
    assert!(h.app.invitation_preview(token).await.is_err());
    assert!(
        h.app
            .accept_invitation(None, new_account(token))
            .await
            .is_err()
    );
    assert!(member(h, owner, w, fresh.user.id).await.active);

    let to_existing = invite(h, owner, second, &fresh.user.email, "viewer").await;
    let existing_token = to_existing.token.as_deref().unwrap();
    assert!(matches!(
        h.app
            .accept_invitation(None, new_account(existing_token))
            .await,
        Err(Error::InviteSignInRequired)
    ));
    assert!(
        h.app
            .accept_invitation(Some(owner), request(json!({"token":existing_token})))
            .await
            .is_err()
    );
    let old_hash: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id=$1")
        .bind(fresh.user.id)
        .fetch_one(&h.owner)
        .await
        .unwrap();
    let (accepted, _) = h
        .app
        .accept_invitation(Some(&fresh), request(json!({"token":existing_token})))
        .await
        .unwrap();
    assert_eq!(accepted.user.id, fresh.user.id);
    assert_eq!(accepted.workspaces.len(), 2);
    let hash: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id=$1")
        .bind(fresh.user.id)
        .fetch_one(&h.owner)
        .await
        .unwrap();
    assert_eq!(
        hash, old_hash,
        "Existing-account acceptance is not password recovery"
    );
    assert_eq!(member(h, owner, second, fresh.user.id).await.role, "viewer");

    let revoked = invite(h, owner, w, "revoked@identity.invalid", "member").await;
    let revoked_now = h
        .app
        .revoke_invitation(owner, w, revoked.invitation.id, &revoked.invitation.etag())
        .await
        .unwrap();
    assert!(revoked_now.revoked_at.is_some());
    assert!(
        h.app
            .accept_invitation(None, new_account(revoked.token.as_deref().unwrap()))
            .await
            .is_err()
    );
    let expired = invite(h, owner, w, "expired@identity.invalid", "member").await;
    let mut tx = h.context(owner.user.id, w).await;
    sqlx::query("UPDATE invitations SET expires_at=now()-interval '1 second' WHERE id=$1")
        .bind(expired.invitation.id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(
        h.app
            .invitation_preview(expired.token.as_deref().unwrap())
            .await
            .is_err()
    );
    assert!(
        h.app
            .accept_invitation(None, new_account(expired.token.as_deref().unwrap()))
            .await
            .is_err()
    );
    let missing_accounts: i64 = sqlx::query_scalar("SELECT count(*) FROM users WHERE email IN ('revoked@identity.invalid','expired@identity.invalid')").fetch_one(&h.owner).await.unwrap();
    assert_eq!(missing_accounts, 0);
    assert!(h.app.invitation_preview("malformed").await.is_err());
    let mut tampered = token.to_string();
    tampered.pop();
    tampered.push(if token.ends_with('0') { '1' } else { '0' });
    assert!(h.app.invitation_preview(&tampered).await.is_err());
    let replaced = invite(h, owner, w, "expired@identity.invalid", "member").await;
    assert_ne!(replaced.invitation.id, expired.invitation.id);
    h.app
        .accept_invitation(None, new_account(replaced.token.as_deref().unwrap()))
        .await
        .unwrap();
}

async fn role_boundaries(
    h: &Harness,
    owner: &Actor,
    admin: &Actor,
    contributor: &Actor,
    viewer: &Actor,
    w: Uuid,
    organization: Uuid,
) {
    assert!(h.app.organizations(contributor).await.unwrap().is_empty());
    assert!(matches!(h.app.create_workspace(contributor,"not-org-admin",request(json!({"organization_id":organization,"name":"Forbidden","team_name":"No","team_key":"NO"}))).await, Err(Error::Forbidden | Error::NotFound)));
    for actor in [contributor, viewer] {
        assert!(matches!(
            h.app
                .create_invitation(
                    actor,
                    w,
                    "denied-invite",
                    request(json!({"email":"denied@identity.invalid","role":"member"}))
                )
                .await,
            Err(Error::Forbidden)
        ));
    }
    assert!(matches!(
        h.app
            .create_invitation(
                admin,
                w,
                "owner-invite",
                request(json!({"email":"owner-escalation@identity.invalid","role":"owner"}))
            )
            .await,
        Err(Error::Invalid(_) | Error::Forbidden)
    ));
    let contributor_before = member(h, owner, w, contributor.user.id).await;
    assert!(matches!(
        h.app
            .update_member(
                admin,
                w,
                contributor.user.id,
                &contributor_before.etag(w),
                request(json!({"role":"owner"}))
            )
            .await,
        Err(Error::Forbidden)
    ));
    let owner_before = member(h, owner, w, owner.user.id).await;
    assert!(matches!(
        h.app
            .update_member(
                admin,
                w,
                owner.user.id,
                &owner_before.etag(w),
                request(json!({"role":"member"}))
            )
            .await,
        Err(Error::Forbidden)
    ));
    assert!(matches!(
        h.app
            .update_member(
                owner,
                w,
                owner.user.id,
                &owner_before.etag(w),
                request(json!({"active":false}))
            )
            .await,
        Err(Error::Conflict(_))
    ));
    let viewer_before = member(h, owner, w, viewer.user.id).await;
    let upgraded = h
        .app
        .update_member(
            admin,
            w,
            viewer.user.id,
            &viewer_before.etag(w),
            request(json!({"role":"member"})),
        )
        .await
        .unwrap();
    assert_eq!(upgraded.entity.role, "member");
    assert!(matches!(
        h.app
            .update_member(
                owner,
                w,
                viewer.user.id,
                &viewer_before.etag(w),
                request(json!({"role":"admin"}))
            )
            .await,
        Err(Error::VersionConflict(_))
    ));
    h.app
        .update_member(
            owner,
            w,
            viewer.user.id,
            &upgraded.entity.etag(w),
            request(json!({"role":"viewer"})),
        )
        .await
        .unwrap();
    let first = h
        .app
        .workspace_members(owner, w, request(json!({"limit":2})))
        .await
        .unwrap();
    assert_eq!(first.data.len(), 2);
    assert!(first.has_more);
    let second = h
        .app
        .workspace_members(
            owner,
            w,
            request(json!({"limit":2,"cursor":first.next_cursor})),
        )
        .await
        .unwrap();
    assert_eq!(second.data.len(), 2);
    assert!(
        !first
            .data
            .iter()
            .any(|a| second.data.iter().any(|b| a.user.id == b.user.id))
    );
    assert!(matches!(
        h.app
            .workspace_members(owner, w, request(json!({"limit":101})))
            .await,
        Err(Error::Invalid(_))
    ));
}

async fn private_team_lifecycle(
    h: &Harness,
    owner: &Actor,
    admin: &Actor,
    contributor: &Actor,
    w: Uuid,
) -> Uuid {
    let body = json!({"name":"Private research","key":"SEC","private":true});
    let team = h
        .app
        .create_team(admin, w, "private-team", request(body.clone()))
        .await
        .unwrap();
    assert!(team.private && team.can_manage);
    assert_eq!(
        h.app
            .create_team(admin, w, "private-team", request(body))
            .await
            .unwrap()
            .id,
        team.id
    );
    assert!(
        !h.app
            .list_teams(owner, w, page())
            .await
            .unwrap()
            .data
            .iter()
            .any(|t| t.id == team.id),
        "Workspace/organization ownership grants no private team visibility"
    );
    assert!(
        !h.app
            .bootstrap(owner, w)
            .await
            .unwrap()
            .teams
            .iter()
            .any(|t| t.id == team.id)
    );
    assert!(matches!(
        h.app.team_members(owner, w, team.id, page()).await,
        Err(Error::NotFound | Error::Forbidden)
    ));
    assert!(matches!(
        h.app
            .update_team(
                owner,
                w,
                team.id,
                &team.etag(),
                request(json!({"name":"Owner intrusion"}))
            )
            .await,
        Err(Error::NotFound | Error::Forbidden)
    ));
    assert!(matches!(
        h.app
            .put_team_member(
                owner,
                w,
                team.id,
                owner.user.id,
                None,
                request(json!({"role":"maintainer"}))
            )
            .await,
        Err(Error::NotFound | Error::Forbidden)
    ));
    let granted = h
        .app
        .put_team_member(
            admin,
            w,
            team.id,
            contributor.user.id,
            None,
            request(json!({"role":"contributor"})),
        )
        .await
        .unwrap();
    assert_eq!(granted.entity.role, "contributor");
    assert!(matches!(
        h.app.session(contributor).await,
        Err(Error::Unauthorized)
    ));
    let contributor = &sign_in_again(h, contributor).await;
    let visible = h.app.list_teams(contributor, w, page()).await.unwrap();
    assert!(
        !visible
            .data
            .iter()
            .find(|t| t.id == team.id)
            .unwrap()
            .can_manage
    );
    assert!(matches!(
        h.app
            .put_team_member(
                contributor,
                w,
                team.id,
                owner.user.id,
                None,
                request(json!({"role":"maintainer"}))
            )
            .await,
        Err(Error::Forbidden)
    ));
    let promoted = h
        .app
        .put_team_member(
            admin,
            w,
            team.id,
            contributor.user.id,
            Some(&granted.entity.etag(team.id)),
            request(json!({"role":"maintainer"})),
        )
        .await
        .unwrap();
    assert_eq!(promoted.entity.role, "maintainer");
    assert!(matches!(
        h.app.session(contributor).await,
        Err(Error::Unauthorized)
    ));
    let contributor = &sign_in_again(h, contributor).await;
    assert!(matches!(
        h.app
            .put_team_member(
                admin,
                w,
                team.id,
                contributor.user.id,
                Some(&granted.entity.etag(team.id)),
                request(json!({"role":"contributor"}))
            )
            .await,
        Err(Error::VersionConflict(_))
    ));
    let created_issue = h
        .app
        .create_issue(contributor, w, "private-issue", issue(team.id))
        .await
        .unwrap();
    h.app
        .create_issue(contributor, w, "private-second-issue", issue(team.id))
        .await
        .unwrap();
    let issue_page = h
        .app
        .list_issues(
            contributor,
            w,
            IssueFilter {
                team_id: Some(team.id),
                limit: Some(1),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(issue_page.has_more);
    let directory_page = h
        .app
        .workspace_members(contributor, w, request(json!({"limit":2})))
        .await
        .unwrap();
    assert!(directory_page.has_more);
    let epoch = h.app.bootstrap(contributor, w).await.unwrap().auth_epoch;
    h.app
        .remove_team_member(
            admin,
            w,
            team.id,
            contributor.user.id,
            &promoted.entity.etag(team.id),
        )
        .await
        .unwrap();
    let bootstrap = h.app.bootstrap(contributor, w).await.unwrap();
    assert_ne!(epoch, bootstrap.auth_epoch);
    assert!(!bootstrap.teams.iter().any(|t| t.id == team.id));
    assert!(!bootstrap.statuses.iter().any(|s| s.team_id == team.id));
    assert!(!bootstrap.issues.iter().any(|i| i.team_id == team.id));
    assert!(matches!(
        h.app.get_issue(contributor, w, created_issue.id).await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        h.app
            .create_issue(contributor, w, "private-after-removal", issue(team.id))
            .await,
        Err(Error::NotFound | Error::Forbidden)
    ));
    assert!(
        matches!(
            h.app
                .list_issues(
                    contributor,
                    w,
                    IssueFilter {
                        team_id: Some(team.id),
                        limit: Some(1),
                        cursor: issue_page.next_cursor,
                        ..Default::default()
                    }
                )
                .await,
            Err(Error::Invalid(_))
        ),
        "Issue cursor must expire when access changes"
    );
    assert!(
        matches!(
            h.app
                .workspace_members(
                    contributor,
                    w,
                    request(json!({"limit":2,"cursor":directory_page.next_cursor}))
                )
                .await,
            Err(Error::Invalid(_))
        ),
        "Directory cursor must expire when access changes"
    );
    let readded = h
        .app
        .put_team_member(
            admin,
            w,
            team.id,
            contributor.user.id,
            None,
            request(json!({"role":"contributor"})),
        )
        .await
        .unwrap();
    assert_ne!(
        readded.entity.etag(team.id),
        promoted.entity.etag(team.id),
        "Removing/readding membership cannot reuse an earlier ETag"
    );
    assert!(matches!(
        h.app
            .remove_team_member(
                admin,
                w,
                team.id,
                contributor.user.id,
                &promoted.entity.etag(team.id)
            )
            .await,
        Err(Error::VersionConflict(_))
    ));
    h.app
        .remove_team_member(
            admin,
            w,
            team.id,
            contributor.user.id,
            &readded.entity.etag(team.id),
        )
        .await
        .unwrap();
    let renamed = h
        .app
        .update_team(
            admin,
            w,
            team.id,
            &team.etag(),
            request(json!({"name":"Research lab"})),
        )
        .await
        .unwrap();
    assert_eq!(renamed.key, team.key);
    assert!(matches!(
        h.app
            .update_team(
                admin,
                w,
                team.id,
                &team.etag(),
                request(json!({"name":"Stale"}))
            )
            .await,
        Err(Error::VersionConflict(_))
    ));
    let archived = h
        .app
        .update_team(
            admin,
            w,
            team.id,
            &renamed.etag(),
            request(json!({"archived":true})),
        )
        .await
        .unwrap();
    assert!(archived.archived_at.is_some());
    let archived_bootstrap = h.app.bootstrap(admin, w).await.unwrap();
    let archived_team = archived_bootstrap
        .teams
        .iter()
        .find(|t| t.id == team.id)
        .expect("Archived team metadata remains readable");
    assert!(archived_team.archived_at.is_some());
    assert!(
        archived_bootstrap
            .statuses
            .iter()
            .any(|s| s.team_id == team.id),
        "Archived issues retain readable status metadata"
    );
    assert!(
        !archived_bootstrap
            .issues
            .iter()
            .any(|i| i.team_id == team.id)
    );
    assert!(
        h.app
            .get_team(admin, w, team.id)
            .await
            .unwrap()
            .archived_at
            .is_some()
    );
    let detail = h.app.get_issue(admin, w, created_issue.id).await.unwrap();
    assert_eq!(detail.title, created_issue.title);
    assert!(
        detail.archived_at.is_none(),
        "Archiving a parent team does not rewrite individual issue history"
    );
    let active_issues = h
        .app
        .list_issues(
            admin,
            w,
            IssueFilter {
                team_id: Some(team.id),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(active_issues.data.is_empty());
    let archive_issues = h
        .app
        .list_issues(
            admin,
            w,
            IssueFilter {
                team_id: Some(team.id),
                archived: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(archive_issues.data.len(), 2);
    assert!(archive_issues.data.iter().any(|i| i.id == created_issue.id));
    assert!(matches!(
        h.app
            .update_issue(
                admin,
                w,
                created_issue.id,
                &detail.etag(),
                json!({"title":"Archived parent edit"})
            )
            .await,
        Err(Error::Conflict(_))
    ));
    assert!(matches!(
        h.app
            .create_issue(admin, w, "archived-team-issue", issue(team.id))
            .await,
        Err(Error::Conflict(_))
    ));
    assert!(matches!(
        h.app
            .create_team(
                admin,
                w,
                "reuse-key",
                request(json!({"name":"Another team","key":"SEC","private":false}))
            )
            .await,
        Err(Error::Conflict(_))
    ));
    h.app
        .update_team(
            admin,
            w,
            team.id,
            &archived.etag(),
            request(json!({"archived":false})),
        )
        .await
        .unwrap();
    let restored_issues = h
        .app
        .list_issues(
            admin,
            w,
            IssueFilter {
                team_id: Some(team.id),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(restored_issues.data.len(), 2);
    assert!(
        restored_issues
            .data
            .iter()
            .any(|i| i.id == created_issue.id)
    );
    assert!(
        h.app
            .list_issues(
                admin,
                w,
                IssueFilter {
                    team_id: Some(team.id),
                    archived: true,
                    ..Default::default()
                }
            )
            .await
            .unwrap()
            .data
            .is_empty()
    );
    assert!(
        h.app
            .bootstrap(admin, w)
            .await
            .unwrap()
            .teams
            .iter()
            .find(|t| t.id == team.id)
            .unwrap()
            .archived_at
            .is_none()
    );
    team.id
}

async fn invitation_authority(h: &Harness, owner: &Actor, admin: &Actor, w: Uuid, private: Uuid) {
    let private_invitation = h.app.create_invitation(admin,w,"private-authority",request(json!({"email":"private-authority@identity.invalid","role":"member","team_ids":[private]}))).await.unwrap();
    let admin_membership = member(h, owner, w, admin.user.id).await;
    let demoted = h
        .app
        .update_member(
            owner,
            w,
            admin.user.id,
            &admin_membership.etag(w),
            request(json!({"role":"member"})),
        )
        .await
        .unwrap();
    assert!(
        h.app
            .accept_invitation(
                None,
                new_account(private_invitation.token.as_deref().unwrap())
            )
            .await
            .is_err(),
        "Invite acceptance must recheck the issuer's current workspace authority"
    );
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM users WHERE email='private-authority@identity.invalid'",
    )
    .fetch_one(&h.owner)
    .await
    .unwrap();
    assert_eq!(
        count, 0,
        "Failed acceptance must roll back new account creation"
    );
    h.app
        .update_member(
            owner,
            w,
            admin.user.id,
            &demoted.entity.etag(w),
            request(json!({"role":"admin"})),
        )
        .await
        .unwrap();
}

async fn blocked_by(h: &Harness, blocker: i32) -> i32 {
    // Observe an actual PostgreSQL lock wait. No test outcome depends on a
    // guessed scheduler delay or on how quickly a command normally completes.
    tokio::time::timeout(std::time::Duration::from_millis(800), async {
        loop {
            let waiter: Option<i32> = sqlx::query_scalar(
                "SELECT pid FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)) LIMIT 1",
            )
            .bind(blocker)
            .fetch_optional(&h.owner)
            .await
            .unwrap();
            if let Some(pid) = waiter {
                return pid;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("The command must enter the expected authorization lock wait")
}

async fn revocation_race(h: &Harness, owner: &Actor, actor: &Actor, w: Uuid, team: Uuid) {
    // Revocation wins: a writer queued behind the exclusive authorization guard
    // must read membership after the revocation commits, with no partial issue.
    let mut guard = h.context(owner.user.id, w).await;
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *guard)
        .await
        .unwrap();
    sqlx::query("SELECT epoch FROM workspace_auth_guards WHERE workspace_id=$1 FOR UPDATE")
        .bind(w)
        .fetch_one(&mut *guard)
        .await
        .unwrap();
    let app = h.app.clone();
    let writer = actor.clone();
    let write = tokio::spawn(async move {
        app.create_issue(&writer, w, "revocation-wins", issue(team))
            .await
    });
    blocked_by(h, blocker).await;
    sqlx::query("UPDATE workspace_members SET active=false,version=version+1 WHERE workspace_id=$1 AND user_id=$2")
        .bind(w).bind(actor.user.id).execute(&mut *guard).await.unwrap();
    sqlx::query("UPDATE workspace_auth_guards SET epoch=epoch+1 WHERE workspace_id=$1")
        .bind(w)
        .execute(&mut *guard)
        .await
        .unwrap();
    guard.commit().await.unwrap();
    assert!(matches!(
        write.await.unwrap(),
        Err(Error::Forbidden | Error::NotFound)
    ));
    let inactive = h
        .app
        .workspace_members(owner, w, request(json!({"state":"inactive"})))
        .await
        .unwrap()
        .data
        .into_iter()
        .find(|m| m.user.id == actor.user.id)
        .unwrap();
    h.app
        .update_member(
            owner,
            w,
            actor.user.id,
            &inactive.etag(w),
            request(json!({"active":true})),
        )
        .await
        .unwrap();

    assert!(matches!(
        h.app.session(actor).await,
        Err(Error::Unauthorized)
    ));
    let actor = &sign_in_again(h, actor).await;
    // The writer wins: while a real issue write holds a shared authorization
    // guard and waits on its team row, a real revocation must wait behind it.
    let mut team_lock = h.context(owner.user.id, w).await;
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *team_lock)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM teams WHERE workspace_id=$1 AND id=$2 FOR UPDATE")
        .bind(w)
        .bind(team)
        .fetch_one(&mut *team_lock)
        .await
        .unwrap();
    let app = h.app.clone();
    let writer = actor.clone();
    let write = tokio::spawn(async move {
        app.create_issue(&writer, w, "writer-wins", issue(team))
            .await
    });
    let writer_pid = blocked_by(h, blocker).await;
    // Read the expected membership version using the fixture transaction. App
    // directory reads acquire the shared guard and remain valid here too.
    let version: String = sqlx::query_scalar(
        "SELECT version::text FROM workspace_members WHERE workspace_id=$1 AND user_id=$2",
    )
    .bind(w)
    .bind(actor.user.id)
    .fetch_one(&mut *team_lock)
    .await
    .unwrap();
    let tag = format!("\"member:{w}:{}:{version}\"", actor.user.id);
    let app = h.app.clone();
    let revoker = owner.clone();
    let target = actor.user.id;
    let revoke = tokio::spawn(async move {
        app.update_member(&revoker, w, target, &tag, request(json!({"active":false})))
            .await
    });
    blocked_by(h, writer_pid).await;
    team_lock.commit().await.unwrap();
    let committed = write
        .await
        .unwrap()
        .expect("Write preceding revocation commits");
    let revoked = revoke
        .await
        .unwrap()
        .expect("Revocation follows the existing write");
    assert!(!revoked.entity.active);
    assert!(h.app.get_issue(owner, w, committed.id).await.is_ok());
    assert!(matches!(
        h.app
            .create_issue(actor, w, "after-race-revocation", issue(team))
            .await,
        Err(Error::Forbidden | Error::NotFound)
    ));
    h.app
        .update_member(
            owner,
            w,
            actor.user.id,
            &revoked.entity.etag(w),
            request(json!({"active":true})),
        )
        .await
        .unwrap();
    let mut tx = h.context(owner.user.id, w).await;
    let writes: i64=sqlx::query_scalar("SELECT count(*) FROM issues WHERE workspace_id=$1 AND team_id=$2 AND created_by_user_id=$3").bind(w).bind(team).bind(actor.user.id).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(
        writes, 1,
        "Only the write linearized before revocation exists"
    );
    tx.commit().await.unwrap();
}

async fn deactivation(
    h: &Harness,
    owner: &Actor,
    contributor: &Actor,
    w: Uuid,
    second: Uuid,
    public_team: Uuid,
) {
    let contributor = &join_existing(h, owner, second, contributor).await;
    let before = member(h, owner, w, contributor.user.id).await;
    let epoch = h.app.bootstrap(owner, w).await.unwrap().auth_epoch;
    let deactivated = h
        .app
        .update_member(
            owner,
            w,
            contributor.user.id,
            &before.etag(w),
            request(json!({"active":false})),
        )
        .await
        .unwrap();
    assert!(!deactivated.entity.active);
    assert_ne!(epoch, h.app.bootstrap(owner, w).await.unwrap().auth_epoch);
    assert!(matches!(
        h.app.bootstrap(contributor, w).await,
        Err(Error::Forbidden | Error::NotFound)
    ));
    assert!(matches!(
        h.app
            .create_issue(contributor, w, "revoked-member-write", issue(public_team))
            .await,
        Err(Error::Forbidden | Error::NotFound)
    ));
    let session = h.app.session(contributor).await.unwrap();
    assert!(!session.workspaces.iter().any(|item| item.id == w));
    assert!(session.workspaces.iter().any(|item| item.id == second));
    h.app.bootstrap(contributor, second).await.unwrap();
    let mut tx = h.context(owner.user.id, w).await;
    let grants: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM team_members WHERE workspace_id=$1 AND user_id=$2",
    )
    .bind(w)
    .bind(contributor.user.id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(grants, 0);
    tx.commit().await.unwrap();
    let reactivated = h
        .app
        .update_member(
            owner,
            w,
            contributor.user.id,
            &deactivated.entity.etag(w),
            request(json!({"active":true})),
        )
        .await
        .unwrap();
    assert!(reactivated.entity.active);
    assert!(matches!(
        h.app.session(contributor).await,
        Err(Error::Unauthorized)
    ));
    let contributor = &sign_in_again(h, contributor).await;
    assert!(
        h.app
            .bootstrap(contributor, w)
            .await
            .unwrap()
            .teams
            .iter()
            .all(|team| !team.private),
        "Reactivation must not silently restore private grants"
    );
}

async fn concurrent_last_owner(h: &Harness, owner: &Actor, admin: &Actor, organization: Uuid) {
    let workspace = h.app.create_workspace(owner,"owner-race-workspace",request(json!({"organization_id":organization,"name":"Owner race","team_name":"Race","team_key":"RACE"}))).await.unwrap();
    let w = workspace.id;
    let admin = &join_existing(h, owner, w, admin).await;
    let before = member(h, owner, w, admin.user.id).await;
    let second_owner = h
        .app
        .update_member(
            owner,
            w,
            admin.user.id,
            &before.etag(w),
            request(json!({"role":"owner"})),
        )
        .await
        .unwrap();
    assert!(matches!(
        h.app.session(admin).await,
        Err(Error::Unauthorized)
    ));
    let admin = &sign_in_again(h, admin).await;
    let first_owner = member(h, owner, w, owner.user.id).await;
    let first_etag = first_owner.etag(w);
    let second_etag = second_owner.entity.etag(w);
    let (first, second) = tokio::join!(
        h.app.update_member(
            owner,
            w,
            owner.user.id,
            &first_etag,
            request(json!({"role":"member"}))
        ),
        h.app.update_member(
            admin,
            w,
            admin.user.id,
            &second_etag,
            request(json!({"role":"member"}))
        )
    );
    assert_eq!(
        usize::from(first.is_ok()) + usize::from(second.is_ok()),
        1,
        "Concurrent self-demotion may remove only one of two owners"
    );
    assert!(matches!(first, Err(Error::Conflict(_))) || matches!(second, Err(Error::Conflict(_))));
    let mut tx = h.context(owner.user.id, w).await;
    let owners: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workspace_members WHERE workspace_id=$1 AND active AND role='owner'",
    )
    .bind(w)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(owners, 1);
    // This also checks the DB invariant independently of the App authorization path.
    let removal = sqlx::query("UPDATE workspace_members SET active=false WHERE workspace_id=$1 AND active AND role='owner'").bind(w).execute(&mut *tx).await;
    removal.unwrap();
    let rejected = tx.commit().await.unwrap_err();
    assert_eq!(
        rejected.as_database_error().unwrap().code().as_deref(),
        Some("23514")
    );
}

async fn recent_authentication(h: &Harness, owner: &Actor, w: Uuid) {
    sqlx::query("UPDATE sessions SET auth_time=now()-interval '11 minutes' WHERE token_hash=$1")
        .bind(&owner.token_hash)
        .execute(&h.owner)
        .await
        .unwrap();
    assert!(matches!(
        h.app
            .create_invitation(
                owner,
                w,
                "old-auth",
                request(json!({"email":"old-auth@identity.invalid","role":"member"}))
            )
            .await,
        Err(Error::ReauthenticationRequired)
    ));
    h.app.bootstrap(owner, w).await.unwrap();
    let (_, cookie) = h
        .app
        .login(request(
            json!({"email":owner.user.email,"password":PASSWORD}),
        ))
        .await
        .unwrap();
    let recent = h.app.authenticate(&cookie).await.unwrap();
    invite(h, &recent, w, "recent-auth@identity.invalid", "member").await;
}
