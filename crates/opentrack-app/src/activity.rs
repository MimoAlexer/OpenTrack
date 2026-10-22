use super::*;
use opentrack_domain::Issue;
use serde_json::{Map, Value, json};

/// Capture non-content facts while the issue's authorized team lock is held.
/// Status names belong to the recorded team audience, which history reads retain.
pub(crate) async fn issue_changes(tx: &mut store::Tx, old: &Issue, new: &Issue) -> Result<Value> {
    let mut changes = Map::new();
    let mut fields = Vec::new();
    if old.title != new.title {
        fields.push("title");
    }
    if old.description != new.description {
        fields.push("description");
    }
    for (field, before, after) in [
        ("status_id", json!(old.status_id), json!(new.status_id)),
        ("priority", json!(old.priority), json!(new.priority)),
        (
            "assignee_id",
            json!(old.assignee_id),
            json!(new.assignee_id),
        ),
        ("label_ids", json!(old.label_ids), json!(new.label_ids)),
        ("estimate", json!(old.estimate), json!(new.estimate)),
        ("due_date", json!(old.due_date), json!(new.due_date)),
        (
            "archived",
            json!(old.archived_at.is_some()),
            json!(new.archived_at.is_some()),
        ),
    ] {
        if before == after {
            continue;
        }
        fields.push(field);
        changes.insert(field.into(), json!({"before":before,"after":after}));
    }
    if old.status_id != new.status_id {
        let (before, after): (String, String) = sqlx::query_as(
            "SELECT previous.name,current.name FROM workflow_statuses previous
             JOIN workflow_statuses current ON current.workspace_id=previous.workspace_id AND current.team_id=previous.team_id
             WHERE previous.workspace_id=$1 AND previous.team_id=$2 AND previous.id=$3 AND current.id=$4",
        )
        .bind(old.workspace_id)
        .bind(old.team_id)
        .bind(old.status_id)
        .bind(new.status_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(db_error)?;
        changes.insert(
            "status_names".into(),
            json!({"before":before,"after":after}),
        );
    }
    changes.insert("fields".into(), json!(fields));
    changes.insert("source".into(), json!("manual"));
    Ok(Value::Object(changes))
}
