//! Changing an existing project lead carries milestone authority and needs a team manager.
#[allow(dead_code)]
mod support;

use opentrack_domain::*;
use serde_json::{Value, json};
use support::Harness;
use uuid::Uuid;

const PASSWORD: &str = "Synthetic project appointment password 2026!";

fn req<T: serde::de::DeserializeOwned>(value: Value) -> T {
    serde_json::from_value(value).unwrap()
}

async fn setup(h: &Harness) -> (Actor, Uuid, Uuid) {
    let (session, token) = h
        .app
        .setup(req(json!({
            "setup_token":h.setup_token,"email":"owner@project-appointment.example.test",
            "display_name":"Example Owner","password":PASSWORD,
            "workspace_name":"Project appointments","team_name":"Engineering","team_key":"ENG"
        })))
        .await
        .unwrap();
    let owner = h.app.authenticate(&token).await.unwrap();
    let w = session.workspaces[0].id;
    let team = h.app.bootstrap(&owner, w).await.unwrap().teams[0].id;
    (owner, w, team)
}

async fn member(h: &Harness, owner: &Actor, w: Uuid, name: &str) -> Actor {
    let issued = h
        .app
        .create_invitation(
            owner,
            w,
            &format!("project-member-{name}"),
            req(
                json!({"email":format!("{name}@project-appointment.example.test"),"role":"member"}),
            ),
        )
        .await
        .unwrap();
    let (_, token) = h
        .app
        .accept_invitation(
            None,
            req(json!({"token":issued.token,"display_name":"Example Member","password":PASSWORD})),
        )
        .await
        .unwrap();
    h.app.authenticate(&token).await.unwrap()
}

async fn project(h: &Harness, actor: &Actor, w: Uuid, t: Uuid, name: &str) -> Project {
    h.app
        .create_project(actor, w, t, name, req(json!({"name":name})))
        .await
        .unwrap()
}

#[tokio::test]
async fn contributors_cannot_appoint_leads_through_either_project_patch_contract() {
    let h = Harness::new().await;
    let copy = h.clone();
    let run = tokio::spawn(async move { patch_permissions(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn patch_permissions(h: &Harness) {
    let (owner, w, t) = setup(h).await;
    let contributor = member(h, &owner, w, "contributor").await;
    let other = member(h, &owner, w, "other").await;
    let initial = project(h, &owner, w, t, "Existing project").await;

    for metadata in [false, true] {
        for proposed in [contributor.user.id, other.user.id] {
            let current = h
                .app
                .get_project(&contributor, w, t, initial.id)
                .await
                .unwrap();
            let patch = req(json!({"lead_id":proposed,"name":"Denied combined edit"}));
            let denied = if metadata {
                h.app
                    .update_project_metadata(
                        &contributor,
                        w,
                        t,
                        current.id,
                        &current.metadata_etag,
                        patch,
                    )
                    .await
                    .map(|_| ())
            } else {
                h.app
                    .update_project(&contributor, w, t, current.id, &current.etag, patch)
                    .await
                    .map(|_| ())
            };
            assert!(matches!(denied, Err(Error::Forbidden)));
            let unchanged = h.app.get_project(&owner, w, t, initial.id).await.unwrap();
            assert_eq!(unchanged.lead_id, None);
            assert_eq!(unchanged.name, initial.name);
            assert_eq!(unchanged.version, initial.version);
        }
    }

    let first = h
        .app
        .create_milestone(
            &owner,
            w,
            t,
            initial.id,
            "first-outcome",
            &initial.etag,
            req(json!({"title":"First outcome"})),
        )
        .await
        .unwrap();
    let current = h.app.get_project(&owner, w, t, initial.id).await.unwrap();
    let second = h
        .app
        .create_milestone(
            &owner,
            w,
            t,
            initial.id,
            "second-outcome",
            &current.etag,
            req(json!({"title":"Second outcome"})),
        )
        .await
        .unwrap();
    let current = h
        .app
        .get_project(&contributor, w, t, initial.id)
        .await
        .unwrap();
    assert!(matches!(
        h.app
            .create_milestone(
                &contributor,
                w,
                t,
                current.id,
                "denied-outcome",
                &current.etag,
                req(json!({"title":"Denied outcome"}))
            )
            .await,
        Err(Error::Forbidden)
    ));
    for patch in [
        json!({"title":"Denied rename"}),
        json!({"target_date":"2026-12-01"}),
        json!({"state":"completed"}),
        json!({"state":"planned"}),
    ] {
        assert!(matches!(
            h.app
                .update_milestone(
                    &contributor,
                    w,
                    t,
                    current.id,
                    first.id,
                    &first.etag,
                    req(patch)
                )
                .await,
            Err(Error::Forbidden)
        ));
    }
    assert!(matches!(
        h.app
            .order_milestones(
                &contributor,
                w,
                t,
                current.id,
                &current.etag,
                MilestoneOrder {
                    milestone_ids: vec![second.id, first.id]
                }
            )
            .await,
        Err(Error::Forbidden)
    ));
    assert!(matches!(
        h.app
            .delete_milestone(&contributor, w, t, current.id, first.id, &first.etag)
            .await,
        Err(Error::Forbidden)
    ));

    // Ordinary metadata edits, including an explicit unchanged empty lead, remain allowed.
    let edited = h
        .app
        .update_project_metadata(
            &contributor,
            w,
            t,
            current.id,
            &current.metadata_etag,
            req(json!({"summary":"Contributor summary","lead_id":null})),
        )
        .await
        .unwrap();
    assert_eq!(edited.summary, "Contributor summary");
    let owner_snapshot = h.app.get_project(&owner, w, t, current.id).await.unwrap();
    let appointed = h
        .app
        .update_project_metadata(
            &owner,
            w,
            t,
            current.id,
            &owner_snapshot.metadata_etag,
            req(json!({"lead_id":contributor.user.id})),
        )
        .await
        .unwrap();
    assert_eq!(appointed.lead_id, Some(contributor.user.id));

    // Existing lead authority does not authorize granting that role to someone else.
    for proposed in [json!(other.user.id), Value::Null] {
        let current = h
            .app
            .get_project(&contributor, w, t, initial.id)
            .await
            .unwrap();
        assert!(matches!(
            h.app
                .update_project_metadata(
                    &contributor,
                    w,
                    t,
                    current.id,
                    &current.metadata_etag,
                    req(json!({"lead_id":proposed}))
                )
                .await,
            Err(Error::Forbidden)
        ));
    }
    let current = h
        .app
        .get_project(&contributor, w, t, initial.id)
        .await
        .unwrap();
    let edited = h
        .app
        .update_project(
            &contributor,
            w,
            t,
            current.id,
            &current.etag,
            req(json!({"lead_id":contributor.user.id,"health":"at_risk"})),
        )
        .await
        .unwrap();
    assert_eq!(edited.health, ProjectHealth::AtRisk);
    let lead_outcome = h
        .app
        .create_milestone(
            &contributor,
            w,
            t,
            current.id,
            "lead-outcome",
            &edited.etag,
            req(json!({"title":"Appointed lead outcome"})),
        )
        .await
        .unwrap();
    assert!(lead_outcome.can_manage);
    let complete = h
        .app
        .update_milestone(
            &contributor,
            w,
            t,
            current.id,
            lead_outcome.id,
            &lead_outcome.etag,
            req(json!({"state":"completed"})),
        )
        .await
        .unwrap();
    let reopened = h
        .app
        .update_milestone(
            &contributor,
            w,
            t,
            current.id,
            complete.id,
            &complete.etag,
            req(json!({"state":"planned","title":"Reviewed outcome"})),
        )
        .await
        .unwrap();
    assert_eq!(reopened.title, "Reviewed outcome");
    let current = h
        .app
        .get_project(&contributor, w, t, initial.id)
        .await
        .unwrap();
    h.app
        .order_milestones(
            &contributor,
            w,
            t,
            current.id,
            &current.etag,
            MilestoneOrder {
                milestone_ids: vec![second.id, first.id, reopened.id],
            },
        )
        .await
        .unwrap();
    let latest = h
        .app
        .get_milestone(&contributor, w, t, current.id, reopened.id)
        .await
        .unwrap();
    h.app
        .delete_milestone(&contributor, w, t, current.id, latest.id, &latest.etag)
        .await
        .unwrap();
}

#[tokio::test]
async fn initial_lead_selection_and_team_manager_appointments_remain_usable() {
    let h = Harness::new().await;
    let copy = h.clone();
    let run = tokio::spawn(async move { create_permissions(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn create_permissions(h: &Harness) {
    let (owner, w, t) = setup(h).await;
    let contributor = member(h, &owner, w, "contributor").await;
    let other = member(h, &owner, w, "other").await;
    for lead in [contributor.user.id, other.user.id] {
        let created = h
            .app
            .create_project(
                &contributor,
                w,
                t,
                &lead.to_string(),
                req(json!({"name":"New contributor project","lead_id":lead})),
            )
            .await
            .unwrap();
        assert_eq!(created.lead_id, Some(lead));
        // Initial selection on a new project does not appoint a lead over existing work.
        assert!(!created.can_manage);
    }
    let ordinary = project(h, &contributor, w, t, "Contributor project").await;
    assert_eq!(ordinary.lead_id, None);
    let replay = project(h, &contributor, w, t, "Contributor project").await;
    assert_eq!(replay.id, ordinary.id);
    let appointed = h
        .app
        .create_project(
            &owner,
            w,
            t,
            "owner-appointment",
            req(json!({"name":"Owner appointment","lead_id":contributor.user.id})),
        )
        .await
        .unwrap();
    assert_eq!(appointed.lead_id, Some(contributor.user.id));
    h.app
        .put_team_member(
            &owner,
            w,
            t,
            other.user.id,
            None,
            req(json!({"role":"maintainer"})),
        )
        .await
        .unwrap();
    let (_, token) = h
        .app
        .login(req(json!({"email":other.user.email,"password":PASSWORD})))
        .await
        .unwrap();
    let maintainer = h.app.authenticate(&token).await.unwrap();
    let managed = h
        .app
        .create_project(
            &maintainer,
            w,
            t,
            "maintainer-appointment",
            req(json!({"name":"Maintainer appointment","lead_id":contributor.user.id})),
        )
        .await
        .unwrap();
    let cleared = h
        .app
        .update_project(
            &maintainer,
            w,
            t,
            managed.id,
            &managed.etag,
            req(json!({"lead_id":null})),
        )
        .await
        .unwrap();
    assert_eq!(cleared.lead_id, None);
    let reassigned = h
        .app
        .update_project_metadata(
            &maintainer,
            w,
            t,
            cleared.id,
            &cleared.metadata_etag,
            req(json!({"lead_id":owner.user.id})),
        )
        .await
        .unwrap();
    assert_eq!(reassigned.lead_id, Some(owner.user.id));
}
