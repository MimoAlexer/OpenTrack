//! Shared transfer capacity is partitioned by authenticated user and workspace.
use crate::ApiError;
use opentrack_domain::Error;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use uuid::Uuid;

const GLOBAL_LIMIT: usize = 8;
const USER_LIMIT: usize = 2;
const WORKSPACE_LIMIT: usize = 4;

pub(super) struct TransferAdmission {
    slots: Arc<Semaphore>,
    counts: Arc<Mutex<Counts>>,
}
#[derive(Default)]
struct Counts {
    users: HashMap<Uuid, usize>,
    workspaces: HashMap<Uuid, usize>,
}
pub(super) struct TransferPermit {
    _global: OwnedSemaphorePermit,
    counts: Arc<Mutex<Counts>>,
    user: Uuid,
    workspace: Uuid,
}
impl Default for TransferAdmission {
    fn default() -> Self {
        Self {
            slots: Arc::new(Semaphore::new(GLOBAL_LIMIT)),
            counts: Arc::new(Mutex::new(Counts::default())),
        }
    }
}
impl TransferAdmission {
    pub(super) fn slot(&self, user: Uuid, workspace: Uuid) -> Result<TransferPermit, ApiError> {
        let global = self
            .slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::RateLimited)?;
        let mut counts = self.counts.lock().map_err(|_| Error::Unavailable)?;
        if counts.users.get(&user).copied().unwrap_or(0) >= USER_LIMIT
            || counts.workspaces.get(&workspace).copied().unwrap_or(0) >= WORKSPACE_LIMIT
        {
            return Err(Error::RateLimited.into());
        }
        *counts.users.entry(user).or_default() += 1;
        *counts.workspaces.entry(workspace).or_default() += 1;
        Ok(TransferPermit {
            _global: global,
            counts: self.counts.clone(),
            user,
            workspace,
        })
    }
    #[cfg(test)]
    pub(super) fn available(&self) -> usize {
        self.slots.available_permits()
    }
}
impl Drop for TransferPermit {
    fn drop(&mut self) {
        // Poisoning fails future admission closed; dropping still releases the
        // global semaphore permit without panicking during cancellation.
        if let Ok(mut counts) = self.counts.lock() {
            release(&mut counts.users, self.user);
            release(&mut counts.workspaces, self.workspace);
        }
    }
}
fn release(counts: &mut HashMap<Uuid, usize>, key: Uuid) {
    if let Some(count) = counts.get_mut(&key) {
        *count -= 1;
        if *count == 0 {
            counts.remove(&key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_limit_spans_workspaces_and_rejected_attempts_release_global_capacity() {
        let admission = TransferAdmission::default();
        let user = Uuid::new_v4();
        let first = admission.slot(user, Uuid::new_v4()).unwrap();
        let second = admission.slot(user, Uuid::new_v4()).unwrap();
        for _ in 0..100 {
            assert!(admission.slot(user, Uuid::new_v4()).is_err());
        }
        assert_eq!(admission.available(), GLOBAL_LIMIT - USER_LIMIT);
        let unrelated = admission.slot(Uuid::new_v4(), Uuid::new_v4()).unwrap();
        drop(first);
        assert!(admission.slot(user, Uuid::new_v4()).is_ok());
        drop((second, unrelated));
        let counts = admission.counts.lock().unwrap();
        assert!(counts.users.is_empty());
        assert!(counts.workspaces.is_empty());
        assert_eq!(admission.available(), GLOBAL_LIMIT);
    }

    #[test]
    fn workspace_limit_leaves_capacity_for_another_workspace() {
        let admission = TransferAdmission::default();
        let workspace = Uuid::new_v4();
        let permits: Vec<_> = (0..WORKSPACE_LIMIT)
            .map(|_| admission.slot(Uuid::new_v4(), workspace).unwrap())
            .collect();
        assert!(admission.slot(Uuid::new_v4(), workspace).is_err());
        assert!(admission.slot(Uuid::new_v4(), Uuid::new_v4()).is_ok());
        drop(permits);
        assert_eq!(admission.available(), GLOBAL_LIMIT);
    }

    #[test]
    fn unrelated_principals_still_share_the_global_cap() {
        let admission = TransferAdmission::default();
        let permits: Vec<_> = (0..GLOBAL_LIMIT)
            .map(|_| admission.slot(Uuid::new_v4(), Uuid::new_v4()).unwrap())
            .collect();
        assert!(admission.slot(Uuid::new_v4(), Uuid::new_v4()).is_err());
        drop(permits);
        assert_eq!(admission.available(), GLOBAL_LIMIT);
    }
}
