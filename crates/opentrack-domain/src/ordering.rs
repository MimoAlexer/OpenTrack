use super::*;

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct IssuePage {
    pub data: Vec<Issue>,
    #[schema(required = true)]
    pub next_cursor: Option<String>,
    pub has_more: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order_version: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub order_maintenance: Vec<IssueOrderMaintenance>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PositionIssue {
    #[schema(min_length = 3, max_length = 39)]
    pub order_version: String,
    #[serde(
        default,
        deserialize_with = "deserialize_present",
        skip_serializing_if = "Option::is_none"
    )]
    #[schema(nullable = false)]
    pub status_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_id: Option<Uuid>,
}
impl PositionIssue {
    pub fn validate(&self, source: Uuid) -> Result<()> {
        let component = |text: &str| matches!(text.parse::<i64>(), Ok(value) if value > 0 && value.to_string() == text);
        if self.order_version.len() > 39
            || !self
                .order_version
                .split_once(':')
                .is_some_and(|(order, workflow)| component(order) && component(workflow))
            || self.before_id == Some(source)
            || self.after_id == Some(source)
            || self.before_id.is_some() && self.before_id == self.after_id
        {
            return Err(Error::Invalid(
                "Invalid issue order version or neighbors".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct IssuePosition {
    pub issue: Issue,
    pub order_version: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum IssueOrderPhase {
    Staging,
    Applying,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct IssueOrderMaintenance {
    pub id: Uuid,
    pub team_id: Uuid,
    pub status_id: Uuid,
    #[schema(value_type = WorkflowJobState)]
    pub state: String,
    pub phase: IssueOrderPhase,
    pub staged_count: String,
    pub applied_count: String,
    #[schema(required = true)]
    pub total_count: Option<String>,
    pub version: String,
    #[schema(required = true, value_type = Option<WorkflowJobError>)]
    pub error_code: Option<String>,
    pub can_resume: bool,
}
impl IssueOrderMaintenance {
    pub fn etag(&self) -> String {
        format!(
            "\"issue-order-maintenance:{}:{}:{}\"",
            self.id, self.version, self.can_resume
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn position_rejects_rank_injection_null_status_and_ambiguous_neighbors() {
        let source = Uuid::new_v4();
        assert!(
            serde_json::from_value::<PositionIssue>(json!({"order_version":"1:1","rank":"1024"}))
                .is_err()
        );
        assert!(
            serde_json::from_value::<PositionIssue>(
                json!({"order_version":"1:1","status_id":null})
            )
            .is_err()
        );
        let mut value: PositionIssue =
            serde_json::from_value(json!({"order_version":"1:1"})).unwrap();
        value.validate(source).unwrap();
        let wire = serde_json::to_value(&value).unwrap();
        assert_eq!(wire, json!({"order_version":"1:1"}));
        serde_json::from_value::<PositionIssue>(wire)
            .unwrap()
            .validate(source)
            .unwrap();
        value.before_id = Some(source);
        assert!(value.validate(source).is_err());
        value.before_id = Some(Uuid::new_v4());
        value.after_id = value.before_id;
        assert!(value.validate(source).is_err());
        value.after_id = Some(Uuid::new_v4());
        value.validate(source).unwrap();
    }

    #[test]
    fn order_versions_are_canonical_positive_bigint_pairs() {
        let source = Uuid::new_v4();
        for version in [
            "",
            "1",
            "0:1",
            "1:0",
            "01:1",
            "1:+1",
            "-1:1",
            "1:1:1",
            " 1:1",
            "1:1\n",
            "1:9223372036854775808",
        ] {
            let value: PositionIssue =
                serde_json::from_value(json!({"order_version":version})).unwrap();
            assert!(value.validate(source).is_err(), "{version:?}");
        }
        for version in ["1:1", "9223372036854775807:9223372036854775807"] {
            let value: PositionIssue =
                serde_json::from_value(json!({"order_version":version})).unwrap();
            value.validate(source).unwrap();
        }
    }

    #[test]
    fn maintenance_etag_changes_with_resume_authority() {
        let mut value = IssueOrderMaintenance {
            id: Uuid::new_v4(),
            team_id: Uuid::new_v4(),
            status_id: Uuid::new_v4(),
            state: "blocked".into(),
            phase: IssueOrderPhase::Staging,
            staged_count: "100".into(),
            applied_count: "0".into(),
            total_count: None,
            version: "2".into(),
            error_code: Some("access_lost".into()),
            can_resume: false,
        };
        let before = value.etag();
        value.can_resume = true;
        assert_ne!(before, value.etag());
        let before = value.etag();
        value.version = "3".into();
        assert_ne!(before, value.etag());
    }

    #[test]
    fn ordinary_issue_pages_keep_the_legacy_shape() {
        let wire = json!({"data":[],"next_cursor":null,"has_more":false});
        let page: IssuePage = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(page).unwrap(), wire);
    }
}
