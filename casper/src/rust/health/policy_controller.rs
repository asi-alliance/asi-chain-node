use std::collections::BTreeMap;

use thiserror::Error;

use crate::rust::health::events::PolicyEvent;
use crate::rust::health::types::{
    EmissionMode, HealthInputSnapshot, HealthRegime, PolicyState, STATUS_FLAG_FALLBACK_INPUT,
};

#[derive(Debug, Default, Clone)]
pub struct HealthPolicyController {
    active_policy: Option<PolicyState>,
    policy_by_epoch: BTreeMap<u64, PolicyState>,
}

impl HealthPolicyController {
    pub fn activate_policy_for_epoch(
        &mut self,
        current_epoch: u64,
        target_epoch: u64,
        snapshot: Option<&HealthInputSnapshot>,
    ) -> Result<PolicyEvent, PolicyControllerError> {
        if target_epoch <= current_epoch {
            return Err(PolicyControllerError::InvalidActivationEpoch {
                current_epoch,
                target_epoch,
            });
        }
        if self.policy_by_epoch.contains_key(&target_epoch) {
            return Err(PolicyControllerError::PolicyAlreadyActivated {
                epoch: target_epoch,
            });
        }

        let source_snapshot_hash = snapshot.map_or([0; 32], |s| s.payload_hash);
        let status_flags = if snapshot.is_none() {
            STATUS_FLAG_FALLBACK_INPUT
        } else {
            0
        };

        let policy = PolicyState {
            epoch_id: target_epoch,
            activation_epoch: target_epoch,
            source_snapshot_hash,
            health_score_ppm: 0,
            health_regime: HealthRegime::Healthy,
            burn_fraction_ppm: 0,
            reserve_emission_rate_ppm: 0,
            emission_mode: EmissionMode::Normal,
            reserve_circuit_breaker: false,
            status_flags,
        };

        self.active_policy = Some(policy.clone());
        self.policy_by_epoch.insert(target_epoch, policy.clone());

        Ok(PolicyEvent::PolicyActivated {
            epoch_id: target_epoch,
            regime: policy.health_regime,
            source_snapshot_hash: policy.source_snapshot_hash,
        })
    }

    pub fn query_current_policy(&self) -> Option<&PolicyState> {
        self.active_policy.as_ref()
    }

    pub fn query_policy_for_epoch(&self, epoch: u64) -> Option<&PolicyState> {
        self.policy_by_epoch.get(&epoch)
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PolicyControllerError {
    #[error("invalid activation epoch: current={current_epoch}, target={target_epoch}")]
    InvalidActivationEpoch {
        current_epoch: u64,
        target_epoch: u64,
    },
    #[error("policy already activated for epoch {epoch}")]
    PolicyAlreadyActivated { epoch: u64 },
}

#[cfg(test)]
mod tests {
    use super::{HealthPolicyController, PolicyControllerError};
    use crate::rust::health::types::STATUS_FLAG_FALLBACK_INPUT;

    #[test]
    fn activate_policy_for_epoch_rejects_current_epoch() {
        let mut controller = HealthPolicyController::default();
        let result = controller.activate_policy_for_epoch(5, 5, None);
        assert_eq!(
            result,
            Err(PolicyControllerError::InvalidActivationEpoch {
                current_epoch: 5,
                target_epoch: 5,
            })
        );
    }

    #[test]
    fn activate_policy_for_epoch_marks_fallback_if_snapshot_is_missing() {
        let mut controller = HealthPolicyController::default();
        let event = controller.activate_policy_for_epoch(5, 6, None);
        assert!(event.is_ok());

        let policy = controller
            .query_current_policy()
            .expect("policy must exist");
        assert_eq!(policy.status_flags, STATUS_FLAG_FALLBACK_INPUT);
    }
}
