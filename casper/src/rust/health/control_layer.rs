use thiserror::Error;

use crate::rust::health::events::PolicyEvent;
use crate::rust::health::input_oracle::{
    HealthInputBundle, HealthInputError, HealthInputOracle, HealthInputOracleConfig,
};
use crate::rust::health::policy_controller::{
    HealthPolicyController, HealthPolicyControllerConfig, PolicyControllerConfigurationError,
    PolicyControllerError,
};
use crate::rust::health::settlement::{
    EpochEconomicSettlement, SettlementConfigurationError, SettlementError, SettlementRoutingConfig,
};
use crate::rust::health::storage::{HealthStateStore, HealthStorageError};
use crate::rust::health::types::{EpochSettlement, HealthInputSnapshot, PolicyState};

#[derive(Clone)]
pub struct HealthControlLayer {
    oracle: HealthInputOracle,
    controller: HealthPolicyController,
    settlement: EpochEconomicSettlement,
    state_store: HealthStateStore,
    event_log: Vec<PolicyEvent>,
}

impl HealthControlLayer {
    pub fn new(
        config: HealthControlLayerConfig,
        state_store: HealthStateStore,
    ) -> Result<Self, HealthControlConfigurationError> {
        let oracle = HealthInputOracle::new(config.oracle, state_store.clone())
            .map_err(HealthControlConfigurationError::Oracle)?;
        let controller = HealthPolicyController::new(config.controller)
            .map_err(HealthControlConfigurationError::Controller)?;
        let settlement = EpochEconomicSettlement::new(config.settlement, state_store.clone())
            .map_err(HealthControlConfigurationError::Settlement)?;
        Ok(Self {
            oracle,
            controller,
            settlement,
            state_store,
            event_log: Vec::new(),
        })
    }

    pub fn submit_health_input(
        &mut self,
        current_epoch: u64,
        bundle: HealthInputBundle,
    ) -> Result<PolicyEvent, HealthControlError> {
        match self.oracle.submit_health_input(current_epoch, bundle) {
            Ok(event) => {
                self.event_log.push(event.clone());
                Ok(event)
            }
            Err(HealthInputError::Rejected(event)) => {
                self.event_log.push(event.clone());
                Err(HealthControlError::Input(HealthInputError::Rejected(event)))
            }
            Err(other) => Err(HealthControlError::Input(other)),
        }
    }

    pub fn activate_policy_for_epoch(
        &mut self,
        current_epoch: u64,
        target_epoch: u64,
    ) -> Result<PolicyEvent, HealthControlError> {
        let pending_snapshot = self.oracle.consume_pending_snapshot(target_epoch)?;
        let event = self.controller.activate_policy_for_epoch(
            current_epoch,
            target_epoch,
            pending_snapshot.as_ref(),
        )?;
        let policy = self
            .controller
            .query_policy_for_epoch(target_epoch)
            .cloned()
            .ok_or(HealthControlError::PolicyNotPersisted {
                epoch_id: target_epoch,
            })?;
        self.state_store.put_policy_state(&policy)?;

        self.event_log.push(event.clone());
        Ok(event)
    }

    pub fn settle_epoch_economics(
        &mut self,
        epoch_id: u64,
        total_fees: u128,
        scheduled_emission: u128,
    ) -> Result<(EpochSettlement, PolicyEvent), HealthControlError> {
        let policy = self
            .state_store
            .get_policy_state(epoch_id)?
            .or_else(|| self.controller.query_policy_for_epoch(epoch_id).cloned())
            .ok_or(HealthControlError::MissingPolicyForEpoch { epoch_id })?;
        let (settlement, event) = self.settlement.settle_epoch_economics(
            epoch_id,
            &policy,
            total_fees,
            scheduled_emission,
        )?;
        self.event_log.push(event.clone());
        Ok((settlement, event))
    }

    pub fn query_current_policy(&self) -> Result<Option<PolicyState>, HealthControlError> {
        if let Some(policy) = self.controller.query_current_policy() {
            return Ok(Some(policy.clone()));
        }
        Ok(self.state_store.get_latest_policy_state()?)
    }

    pub fn query_pending_policy(
        &self,
        target_epoch: u64,
    ) -> Result<Option<HealthInputSnapshot>, HealthControlError> {
        Ok(self.state_store.get_pending_snapshot(target_epoch)?)
    }

    pub fn query_last_settlement(&self) -> Result<Option<EpochSettlement>, HealthControlError> {
        Ok(self.state_store.get_last_settlement()?)
    }

    pub fn events(&self) -> &[PolicyEvent] {
        &self.event_log
    }

    pub fn events_since(&self, start_index: usize) -> &[PolicyEvent] {
        self.event_log.get(start_index..).unwrap_or(&[])
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthControlLayerConfig {
    pub oracle: HealthInputOracleConfig,
    pub controller: HealthPolicyControllerConfig,
    pub settlement: SettlementRoutingConfig,
}

impl Default for HealthControlLayerConfig {
    fn default() -> Self {
        Self {
            oracle: HealthInputOracleConfig::default(),
            controller: HealthPolicyControllerConfig::default(),
            settlement: SettlementRoutingConfig::default(),
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum HealthControlConfigurationError {
    #[error("health input oracle configuration error: {0}")]
    Oracle(#[from] crate::rust::health::input_oracle::HealthInputConfigurationError),
    #[error("policy controller configuration error: {0}")]
    Controller(#[from] PolicyControllerConfigurationError),
    #[error("settlement configuration error: {0}")]
    Settlement(#[from] SettlementConfigurationError),
}

#[derive(Debug, Error)]
pub enum HealthControlError {
    #[error("health input error: {0}")]
    Input(#[from] HealthInputError),
    #[error("policy controller error: {0}")]
    PolicyController(#[from] PolicyControllerError),
    #[error("settlement error: {0}")]
    Settlement(#[from] SettlementError),
    #[error("storage error: {0}")]
    Storage(#[from] HealthStorageError),
    #[error("policy state for epoch {epoch_id} was not persisted after activation")]
    PolicyNotPersisted { epoch_id: u64 },
    #[error("no policy found for epoch {epoch_id}")]
    MissingPolicyForEpoch { epoch_id: u64 },
}

#[cfg(test)]
mod tests {
    use super::{HealthControlError, HealthControlLayer, HealthControlLayerConfig};
    use crate::rust::health::events::PolicyEvent;
    use crate::rust::health::input_oracle::{
        encode_metrics_payload_v1, HealthInputBundle, HealthInputSignature, HealthMetricsPayloadV1,
    };
    use crate::rust::health::storage::HealthStateStore;
    use crate::rust::health::types::HealthRegime;

    fn control_layer() -> HealthControlLayer {
        HealthControlLayer::new(
            HealthControlLayerConfig::default(),
            HealthStateStore::in_memory_for_tests(),
        )
        .expect("config must be valid")
    }

    fn bundle(target_epoch: u64, psi_ppm: u64, fee_quality_ppm: u64) -> HealthInputBundle {
        let payload_bytes = encode_metrics_payload_v1(&HealthMetricsPayloadV1 {
            psi_ppm,
            fee_quality_ppm,
        })
        .expect("payload encode");
        let payload_hash = {
            let digest = crypto::rust::hash::blake2b256::Blake2b256::hash(payload_bytes.clone());
            let mut out = [0u8; 32];
            out.copy_from_slice(&digest[..32]);
            out
        };
        HealthInputBundle {
            target_epoch,
            submitted_at_millis: 1000,
            metrics_version: 1,
            payload_hash,
            payload_bytes,
            signer_set_id: 1,
            signatures: vec![
                HealthInputSignature {
                    signer: "oracle-1".to_string(),
                    sig_bytes: vec![1, 2, 3],
                },
                HealthInputSignature {
                    signer: "oracle-2".to_string(),
                    sig_bytes: vec![4, 5, 6],
                },
            ],
        }
    }

    #[test]
    fn submit_activate_settle_flow_is_observable() {
        let mut control = control_layer();

        let submit_event = control
            .submit_health_input(10, bundle(11, 900_000, 900_000))
            .expect("submit should succeed");
        assert!(matches!(
            submit_event,
            PolicyEvent::HealthInputAccepted { .. }
        ));

        let activate_event = control
            .activate_policy_for_epoch(10, 11)
            .expect("activate should succeed");
        assert!(matches!(
            activate_event,
            PolicyEvent::PolicyActivated { .. }
        ));

        let (record, settle_event) = control
            .settle_epoch_economics(11, 10_000, 1_000)
            .expect("settlement should succeed");
        assert_eq!(record.epoch_id, 11);
        assert!(matches!(settle_event, PolicyEvent::EpochSettled { .. }));
        assert_eq!(control.events().len(), 3);
    }

    #[test]
    fn activate_without_pending_bundle_uses_fallback_policy() {
        let mut control = control_layer();

        control
            .activate_policy_for_epoch(10, 11)
            .expect("fallback activation should succeed");
        let current = control
            .query_current_policy()
            .expect("query should succeed")
            .expect("policy should exist");
        assert_eq!(current.health_regime, HealthRegime::Healthy);
        assert_ne!(
            current.status_flags & crate::rust::health::types::STATUS_FLAG_FALLBACK_INPUT,
            0
        );
    }

    #[test]
    fn event_log_includes_rejections() {
        let mut control = control_layer();

        let rejected = control.submit_health_input(10, bundle(10, 900_000, 900_000));
        assert!(matches!(
            rejected,
            Err(HealthControlError::Input(
                crate::rust::health::input_oracle::HealthInputError::Rejected(
                    PolicyEvent::HealthInputRejected { .. }
                )
            ))
        ));
        assert_eq!(control.events().len(), 1);
        assert!(matches!(
            control.events()[0],
            PolicyEvent::HealthInputRejected { .. }
        ));
    }

    #[test]
    fn settlement_without_policy_is_rejected() {
        let mut control = control_layer();
        let result = control.settle_epoch_economics(22, 100, 10);
        assert!(matches!(
            result,
            Err(HealthControlError::MissingPolicyForEpoch { epoch_id: 22 })
        ));
    }
}
