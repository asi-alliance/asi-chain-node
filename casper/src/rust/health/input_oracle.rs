use std::collections::BTreeMap;

use thiserror::Error;

use crate::rust::health::events::{HealthInputRejectionReason, PolicyEvent};
use crate::rust::health::types::HealthInputSnapshot;

#[derive(Debug, Default, Clone)]
pub struct HealthInputOracle {
    pending_by_epoch: BTreeMap<u64, HealthInputSnapshot>,
}

impl HealthInputOracle {
    pub fn submit_health_input(
        &mut self,
        current_epoch: u64,
        snapshot: HealthInputSnapshot,
    ) -> Result<PolicyEvent, HealthInputError> {
        if !snapshot.is_for_future_epoch(current_epoch) {
            return Err(HealthInputError::Rejected(
                PolicyEvent::HealthInputRejected {
                    target_epoch: snapshot.target_epoch,
                    reason: HealthInputRejectionReason::InvalidTargetEpoch,
                },
            ));
        }

        if snapshot.metrics_version == 0 || snapshot.payload_bytes.is_empty() {
            return Err(HealthInputError::Rejected(
                PolicyEvent::HealthInputRejected {
                    target_epoch: snapshot.target_epoch,
                    reason: HealthInputRejectionReason::InvalidSchema,
                },
            ));
        }

        if !snapshot.quorum_verified || !snapshot.accepted {
            return Err(HealthInputError::Rejected(
                PolicyEvent::HealthInputRejected {
                    target_epoch: snapshot.target_epoch,
                    reason: HealthInputRejectionReason::InsufficientQuorum,
                },
            ));
        }

        if self.pending_by_epoch.contains_key(&snapshot.target_epoch) {
            return Err(HealthInputError::Rejected(
                PolicyEvent::HealthInputRejected {
                    target_epoch: snapshot.target_epoch,
                    reason: HealthInputRejectionReason::DuplicateSubmission,
                },
            ));
        }

        self.pending_by_epoch
            .insert(snapshot.target_epoch, snapshot.clone());

        Ok(PolicyEvent::HealthInputAccepted {
            target_epoch: snapshot.target_epoch,
            payload_hash: snapshot.payload_hash,
        })
    }

    pub fn get_pending_snapshot(&self, target_epoch: u64) -> Option<&HealthInputSnapshot> {
        self.pending_by_epoch.get(&target_epoch)
    }

    pub fn activate_snapshot(&mut self, target_epoch: u64) -> Option<HealthInputSnapshot> {
        self.pending_by_epoch.remove(&target_epoch)
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum HealthInputError {
    #[error("health input rejected")]
    Rejected(PolicyEvent),
}

#[cfg(test)]
mod tests {
    use super::{HealthInputError, HealthInputOracle};
    use crate::rust::health::events::{HealthInputRejectionReason, PolicyEvent};
    use crate::rust::health::types::HealthInputSnapshot;

    fn snapshot(target_epoch: u64) -> HealthInputSnapshot {
        HealthInputSnapshot {
            target_epoch,
            submitted_at_millis: 1000,
            metrics_version: 1,
            payload_hash: [7; 32],
            payload_bytes: vec![1, 2, 3],
            signer_set_id: 1,
            quorum_verified: true,
            accepted: true,
        }
    }

    #[test]
    fn submit_health_input_rejects_non_future_epoch() {
        let mut oracle = HealthInputOracle::default();
        let result = oracle.submit_health_input(10, snapshot(10));

        assert_eq!(
            result,
            Err(HealthInputError::Rejected(
                PolicyEvent::HealthInputRejected {
                    target_epoch: 10,
                    reason: HealthInputRejectionReason::InvalidTargetEpoch,
                }
            ))
        );
    }

    #[test]
    fn submit_health_input_rejects_duplicate_submission() {
        let mut oracle = HealthInputOracle::default();
        let first = oracle.submit_health_input(10, snapshot(11));
        assert!(first.is_ok());

        let second = oracle.submit_health_input(10, snapshot(11));
        assert_eq!(
            second,
            Err(HealthInputError::Rejected(
                PolicyEvent::HealthInputRejected {
                    target_epoch: 11,
                    reason: HealthInputRejectionReason::DuplicateSubmission,
                }
            ))
        );
    }
}
