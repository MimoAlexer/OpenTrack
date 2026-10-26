use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlanningCalculationSource {
    #[default]
    Live,
    Projection,
    Snapshot,
}

pub(crate) fn snapshot_calculation_source() -> PlanningCalculationSource {
    PlanningCalculationSource::Snapshot
}

#[derive(Debug, Clone, Copy)]
pub struct PlanningBucket<'a> {
    pub category: &'a str,
    pub excluded: bool,
    pub count: i64,
    pub estimate_total: Option<i64>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct PlanningCounts {
    pub total: i64,
    pub completed: i64,
    pub canceled: i64,
    pub excluded: i64,
    pub estimate_total: i64,
}

impl PlanningCounts {
    pub fn from_buckets<'a>(buckets: impl IntoIterator<Item = PlanningBucket<'a>>) -> Self {
        let mut counts = Self::default();
        for bucket in buckets {
            if bucket.excluded {
                counts.excluded += bucket.count;
                continue;
            }
            counts.total += bucket.count;
            match bucket.category {
                "completed" => counts.completed += bucket.count,
                "canceled" => counts.canceled += bucket.count,
                _ => {}
            }
            if bucket.category != "canceled" {
                counts.estimate_total += bucket.estimate_total.unwrap_or(0);
            }
        }
        counts
    }

    pub fn eligible(&self) -> i64 {
        self.total - self.canceled
    }

    pub fn unfinished(&self) -> i64 {
        self.eligible() - self.completed
    }

    pub fn unscoped(&self) -> bool {
        self.eligible() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn planning_counts_preserve_scope_categories_exclusions_and_estimates() {
        let buckets = [
            ("backlog", false, 2, Some(3)),
            ("unstarted", false, 1, None),
            ("started", false, 3, Some(8)),
            ("completed", false, 2, Some(5)),
            ("canceled", false, 4, Some(40)),
            ("completed", true, 2, Some(20)),
            ("started", true, 1, Some(10)),
            ("canceled", true, 1, None),
        ];
        let counts = PlanningCounts::from_buckets(buckets.map(
            |(category, excluded, count, estimate_total)| PlanningBucket {
                category,
                excluded,
                count,
                estimate_total,
            },
        ));
        assert_eq!(
            counts,
            PlanningCounts {
                total: 12,
                completed: 2,
                canceled: 4,
                excluded: 4,
                estimate_total: 16,
            }
        );
        assert_eq!(counts.eligible(), 8);
        assert_eq!(counts.unfinished(), 6);
        assert!(!counts.unscoped());
    }

    #[test]
    fn empty_excluded_only_and_canceled_only_scopes_are_unscoped() {
        let empty = PlanningCounts::from_buckets([]);
        assert_eq!(empty, PlanningCounts::default());
        assert!(empty.unscoped());
        for (excluded, canceled) in [(true, 0), (false, 3)] {
            let counts = PlanningCounts::from_buckets([PlanningBucket {
                category: "canceled",
                excluded,
                count: 3,
                estimate_total: Some(30),
            }]);
            assert_eq!(counts.canceled, canceled);
            assert_eq!(counts.estimate_total, 0);
            assert_eq!(counts.eligible(), 0);
            assert_eq!(counts.unfinished(), 0);
            assert!(counts.unscoped());
        }
    }

    #[test]
    fn grouped_and_individual_facts_have_identical_metrics() {
        let individual =
            PlanningCounts::from_buckets([Some(0), None, Some(7)].map(|estimate_total| {
                PlanningBucket {
                    category: "started",
                    excluded: false,
                    count: 1,
                    estimate_total,
                }
            }));
        let grouped = PlanningCounts::from_buckets([PlanningBucket {
            category: "started",
            excluded: false,
            count: 3,
            estimate_total: Some(7),
        }]);
        assert_eq!(individual, grouped);
    }
}
