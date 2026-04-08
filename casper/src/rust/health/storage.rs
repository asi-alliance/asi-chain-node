use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use shared::rust::store::key_value_store::KvStoreError;
use shared::rust::store::key_value_typed_store::KeyValueTypedStore;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;
use thiserror::Error;

use crate::rust::health::codec::{
    deserialize_epoch_settlement, deserialize_health_input_snapshot, deserialize_policy_parameters,
    deserialize_policy_state, serialize_epoch_settlement, serialize_health_input_snapshot,
    serialize_policy_parameters, serialize_policy_state, HealthCodecError,
};
use crate::rust::health::params::PolicyParameters;
use crate::rust::health::types::{EpochSettlement, HealthInputSnapshot, PolicyState};

pub const HEALTH_INPUT_SNAPSHOT_STORE_NAME: &str = "health-input-snapshots";
pub const HEALTH_POLICY_STATE_STORE_NAME: &str = "health-policy-states";
pub const HEALTH_EPOCH_SETTLEMENT_STORE_NAME: &str = "health-epoch-settlements";
pub const HEALTH_POLICY_PARAMETERS_STORE_NAME: &str = "health-policy-parameters";

const ACTIVE_POLICY_PARAMETERS_KEY: &str = "active";

type EpochBytesStore = KeyValueTypedStoreImpl<u64, Vec<u8>>;
type StringBytesStore = KeyValueTypedStoreImpl<String, Vec<u8>>;

#[derive(Clone)]
pub struct HealthStateStore {
    input_snapshot_store: EpochBytesStore,
    policy_state_store: EpochBytesStore,
    settlement_store: EpochBytesStore,
    policy_parameters_store: StringBytesStore,
}

impl HealthStateStore {
    pub async fn create(
        store_manager: &mut dyn KeyValueStoreManager,
    ) -> Result<Self, HealthStorageError> {
        let input_snapshot_store = store_manager
            .store(HEALTH_INPUT_SNAPSHOT_STORE_NAME.to_string())
            .await
            .map(EpochBytesStore::new)
            .map_err(|source| HealthStorageError::StoreInit {
                store_name: HEALTH_INPUT_SNAPSHOT_STORE_NAME,
                message: source.to_string(),
            })?;
        let policy_state_store = store_manager
            .store(HEALTH_POLICY_STATE_STORE_NAME.to_string())
            .await
            .map(EpochBytesStore::new)
            .map_err(|source| HealthStorageError::StoreInit {
                store_name: HEALTH_POLICY_STATE_STORE_NAME,
                message: source.to_string(),
            })?;
        let settlement_store = store_manager
            .store(HEALTH_EPOCH_SETTLEMENT_STORE_NAME.to_string())
            .await
            .map(EpochBytesStore::new)
            .map_err(|source| HealthStorageError::StoreInit {
                store_name: HEALTH_EPOCH_SETTLEMENT_STORE_NAME,
                message: source.to_string(),
            })?;
        let policy_parameters_store = store_manager
            .store(HEALTH_POLICY_PARAMETERS_STORE_NAME.to_string())
            .await
            .map(StringBytesStore::new)
            .map_err(|source| HealthStorageError::StoreInit {
                store_name: HEALTH_POLICY_PARAMETERS_STORE_NAME,
                message: source.to_string(),
            })?;

        Ok(Self {
            input_snapshot_store,
            policy_state_store,
            settlement_store,
            policy_parameters_store,
        })
    }

    pub fn put_pending_snapshot(
        &self,
        snapshot: &HealthInputSnapshot,
    ) -> Result<(), HealthStorageError> {
        let bytes = serialize_health_input_snapshot(snapshot)?;
        self.input_snapshot_store
            .put_one(snapshot.target_epoch, bytes)?;
        Ok(())
    }

    pub fn get_pending_snapshot(
        &self,
        target_epoch: u64,
    ) -> Result<Option<HealthInputSnapshot>, HealthStorageError> {
        self.input_snapshot_store
            .get_one(&target_epoch)?
            .map(|bytes| deserialize_health_input_snapshot(&bytes))
            .transpose()
            .map_err(HealthStorageError::from)
    }

    pub fn put_policy_state(&self, policy_state: &PolicyState) -> Result<(), HealthStorageError> {
        let bytes = serialize_policy_state(policy_state)?;
        self.policy_state_store
            .put_one(policy_state.epoch_id, bytes)?;
        Ok(())
    }

    pub fn get_policy_state(
        &self,
        epoch_id: u64,
    ) -> Result<Option<PolicyState>, HealthStorageError> {
        self.policy_state_store
            .get_one(&epoch_id)?
            .map(|bytes| deserialize_policy_state(&bytes))
            .transpose()
            .map_err(HealthStorageError::from)
    }

    pub fn get_latest_policy_state(&self) -> Result<Option<PolicyState>, HealthStorageError> {
        let state_map = self.policy_state_store.to_map()?;
        if let Some((_, encoded)) = state_map.into_iter().max_by_key(|(epoch, _)| *epoch) {
            return Ok(Some(deserialize_policy_state(&encoded)?));
        }
        Ok(None)
    }

    pub fn put_epoch_settlement(
        &self,
        settlement: &EpochSettlement,
    ) -> Result<(), HealthStorageError> {
        let bytes = serialize_epoch_settlement(settlement)?;
        self.settlement_store.put_one(settlement.epoch_id, bytes)?;
        Ok(())
    }

    pub fn get_epoch_settlement(
        &self,
        epoch_id: u64,
    ) -> Result<Option<EpochSettlement>, HealthStorageError> {
        self.settlement_store
            .get_one(&epoch_id)?
            .map(|bytes| deserialize_epoch_settlement(&bytes))
            .transpose()
            .map_err(HealthStorageError::from)
    }

    pub fn get_last_settlement(&self) -> Result<Option<EpochSettlement>, HealthStorageError> {
        let settlement_map = self.settlement_store.to_map()?;
        if let Some((_, encoded)) = settlement_map.into_iter().max_by_key(|(epoch, _)| *epoch) {
            return Ok(Some(deserialize_epoch_settlement(&encoded)?));
        }
        Ok(None)
    }

    pub fn put_policy_parameters(
        &self,
        params: &PolicyParameters,
    ) -> Result<(), HealthStorageError> {
        let bytes = serialize_policy_parameters(params)?;
        self.policy_parameters_store
            .put_one(ACTIVE_POLICY_PARAMETERS_KEY.to_string(), bytes)?;
        Ok(())
    }

    pub fn get_policy_parameters(&self) -> Result<Option<PolicyParameters>, HealthStorageError> {
        self.policy_parameters_store
            .get_one(&ACTIVE_POLICY_PARAMETERS_KEY.to_string())?
            .map(|bytes| deserialize_policy_parameters(&bytes))
            .transpose()
            .map_err(HealthStorageError::from)
    }

    #[cfg(test)]
    fn from_stores(
        input_snapshot_store: EpochBytesStore,
        policy_state_store: EpochBytesStore,
        settlement_store: EpochBytesStore,
        policy_parameters_store: StringBytesStore,
    ) -> Self {
        Self {
            input_snapshot_store,
            policy_state_store,
            settlement_store,
            policy_parameters_store,
        }
    }
}

#[derive(Debug, Error)]
pub enum HealthStorageError {
    #[error("failed to initialize health store '{store_name}': {message}")]
    StoreInit {
        store_name: &'static str,
        message: String,
    },
    #[error("typed key-value store failure: {0}")]
    KvStore(#[from] KvStoreError),
    #[error(transparent)]
    Codec(#[from] HealthCodecError),
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
    use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

    use super::HealthStateStore;
    use crate::rust::health::params::PolicyParameters;
    use crate::rust::health::types::{
        EmissionMode, EpochSettlement, HealthInputSnapshot, HealthRegime, PolicyState,
    };

    fn epoch_store() -> KeyValueTypedStoreImpl<u64, Vec<u8>> {
        KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new()))
    }

    fn string_store() -> KeyValueTypedStoreImpl<String, Vec<u8>> {
        KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new()))
    }

    fn state_store() -> HealthStateStore {
        HealthStateStore::from_stores(epoch_store(), epoch_store(), epoch_store(), string_store())
    }

    #[test]
    fn storage_round_trips_pending_snapshot() {
        let store = state_store();
        let snapshot = HealthInputSnapshot {
            target_epoch: 10,
            submitted_at_millis: 111,
            metrics_version: 1,
            payload_hash: [2; 32],
            payload_bytes: vec![9, 8, 7],
            signer_set_id: 1,
            quorum_verified: true,
            accepted: true,
        };

        store.put_pending_snapshot(&snapshot).expect("must store");
        let loaded = store
            .get_pending_snapshot(snapshot.target_epoch)
            .expect("must load")
            .expect("must exist");
        assert_eq!(loaded, snapshot);
    }

    #[test]
    fn storage_returns_latest_policy_and_settlement() {
        let store = state_store();

        let policy_10 = PolicyState {
            epoch_id: 10,
            activation_epoch: 10,
            source_snapshot_hash: [1; 32],
            health_score_ppm: 100,
            health_regime: HealthRegime::Cautious,
            burn_fraction_ppm: 100_000,
            reserve_emission_rate_ppm: 50_000,
            emission_mode: EmissionMode::Normal,
            reserve_circuit_breaker: false,
            status_flags: 0,
        };
        let policy_11 = PolicyState {
            epoch_id: 11,
            activation_epoch: 11,
            source_snapshot_hash: [2; 32],
            health_score_ppm: 200,
            health_regime: HealthRegime::Stressed,
            burn_fraction_ppm: 200_000,
            reserve_emission_rate_ppm: 25_000,
            emission_mode: EmissionMode::Hibernating,
            reserve_circuit_breaker: true,
            status_flags: 1,
        };

        store
            .put_policy_state(&policy_10)
            .expect("must store policy");
        store
            .put_policy_state(&policy_11)
            .expect("must store policy");

        let latest_policy = store
            .get_latest_policy_state()
            .expect("must load latest policy")
            .expect("latest policy must exist");
        assert_eq!(latest_policy, policy_11);

        let settlement_10 = EpochSettlement {
            epoch_id: 10,
            total_fees: 1000,
            burned_amount: 100,
            validator_reward_amount: 900,
            staking_reward_amount: 500,
            treasury_amount: 100,
            insurance_amount: 100,
            reserve_emission_amount: 200,
            minted_emission_amount: 700,
            hibernation_applied: false,
            settlement_hash: [4; 32],
        };
        let settlement_12 = EpochSettlement {
            epoch_id: 12,
            total_fees: 2000,
            burned_amount: 200,
            validator_reward_amount: 1800,
            staking_reward_amount: 800,
            treasury_amount: 200,
            insurance_amount: 200,
            reserve_emission_amount: 400,
            minted_emission_amount: 1200,
            hibernation_applied: true,
            settlement_hash: [5; 32],
        };

        store
            .put_epoch_settlement(&settlement_10)
            .expect("must store settlement");
        store
            .put_epoch_settlement(&settlement_12)
            .expect("must store settlement");

        let last_settlement = store
            .get_last_settlement()
            .expect("must load last settlement")
            .expect("last settlement must exist");
        assert_eq!(last_settlement, settlement_12);
    }

    #[test]
    fn storage_round_trips_policy_parameters() {
        let store = state_store();
        let params = PolicyParameters {
            burn_fraction_min_ppm: 10_000,
            burn_fraction_max_ppm: 200_000,
            reserve_emission_min_ppm: 0,
            reserve_emission_max_ppm: 500_000,
            min_regime_dwell_epochs: 2,
            upward_transition_hysteresis_ppm: 5_000,
            downward_transition_hysteresis_ppm: 10_000,
            allow_emergency_override: false,
        };

        store
            .put_policy_parameters(&params)
            .expect("must store parameters");
        let loaded = store
            .get_policy_parameters()
            .expect("must load parameters")
            .expect("parameters must exist");
        assert_eq!(loaded, params);
    }
}
