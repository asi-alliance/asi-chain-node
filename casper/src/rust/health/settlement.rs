use crypto::rust::hash::blake2b256::{Blake2b256, HASH_LENGTH};
use thiserror::Error;

use crate::rust::health::events::PolicyEvent;
use crate::rust::health::storage::{HealthStateStore, HealthStorageError};
use crate::rust::health::types::{EmissionMode, EpochSettlement, PolicyState, PPM_DENOMINATOR};

#[derive(Clone)]
pub struct EpochEconomicSettlement {
    routing_config: SettlementRoutingConfig,
    state_store: HealthStateStore,
}

impl EpochEconomicSettlement {
    pub fn new(
        routing_config: SettlementRoutingConfig,
        state_store: HealthStateStore,
    ) -> Result<Self, SettlementConfigurationError> {
        routing_config.validate()?;
        Ok(Self {
            routing_config,
            state_store,
        })
    }

    pub fn settle_epoch_economics(
        &self,
        epoch_id: u64,
        policy: &PolicyState,
        total_fees: u128,
        scheduled_emission: u128,
    ) -> Result<(EpochSettlement, PolicyEvent), SettlementError> {
        if self.state_store.get_epoch_settlement(epoch_id)?.is_some() {
            return Err(SettlementError::SettlementAlreadyExists { epoch_id });
        }
        if policy.epoch_id != epoch_id {
            return Err(SettlementError::PolicyEpochMismatch {
                policy_epoch: policy.epoch_id,
                settlement_epoch: epoch_id,
            });
        }

        let burned_amount = ppm_mul_floor(total_fees, policy.burn_fraction_ppm)?;
        let net_fees = total_fees
            .checked_sub(burned_amount)
            .ok_or(SettlementError::AccountingUnderflow)?;
        let hibernation_applied = matches!(policy.emission_mode, EmissionMode::Hibernating);
        let minted_emission_amount = if hibernation_applied {
            0
        } else {
            scheduled_emission
        };
        let reserve_emission_amount =
            ppm_mul_floor(minted_emission_amount, policy.reserve_emission_rate_ppm)?;
        let distributable_emission = minted_emission_amount
            .checked_sub(reserve_emission_amount)
            .ok_or(SettlementError::AccountingUnderflow)?;

        let fee_routes = route_amount(
            net_fees,
            self.routing_config.validator_fee_share_ppm,
            self.routing_config.staking_fee_share_ppm,
            self.routing_config.treasury_fee_share_ppm,
        )?;
        let emission_routes = route_amount(
            distributable_emission,
            self.routing_config.validator_emission_share_ppm,
            self.routing_config.staking_emission_share_ppm,
            self.routing_config.treasury_emission_share_ppm,
        )?;

        let validator_reward_amount = fee_routes
            .validator
            .checked_add(emission_routes.validator)
            .ok_or(SettlementError::AccountingOverflow)?;
        let staking_reward_amount = fee_routes
            .staking
            .checked_add(emission_routes.staking)
            .ok_or(SettlementError::AccountingOverflow)?;
        let treasury_amount = fee_routes
            .treasury
            .checked_add(emission_routes.treasury)
            .ok_or(SettlementError::AccountingOverflow)?;
        let insurance_amount = fee_routes
            .insurance
            .checked_add(emission_routes.insurance)
            .ok_or(SettlementError::AccountingOverflow)?;

        let mut settlement = EpochSettlement {
            epoch_id,
            total_fees,
            burned_amount,
            validator_reward_amount,
            staking_reward_amount,
            treasury_amount,
            insurance_amount,
            reserve_emission_amount,
            minted_emission_amount,
            hibernation_applied,
            settlement_hash: [0; HASH_LENGTH],
        };
        settlement.settlement_hash = settlement_hash(&settlement);

        self.state_store.put_epoch_settlement(&settlement)?;

        let spent = settlement
            .burned_amount
            .checked_add(settlement.validator_reward_amount)
            .and_then(|value| value.checked_add(settlement.staking_reward_amount))
            .and_then(|value| value.checked_add(settlement.treasury_amount))
            .and_then(|value| value.checked_add(settlement.insurance_amount))
            .and_then(|value| value.checked_add(settlement.reserve_emission_amount))
            .ok_or(SettlementError::AccountingOverflow)?;
        let funded = settlement
            .total_fees
            .checked_add(settlement.minted_emission_amount)
            .ok_or(SettlementError::AccountingOverflow)?;
        if spent != funded {
            return Err(SettlementError::AccountingInvariantViolation { spent, funded });
        }

        let event = PolicyEvent::EpochSettled {
            epoch_id,
            burned_amount: settlement.burned_amount,
            minted_emission_amount: settlement.minted_emission_amount,
            hibernation_applied: settlement.hibernation_applied,
        };
        Ok((settlement, event))
    }

    pub fn query_last_settlement(&self) -> Result<Option<EpochSettlement>, SettlementError> {
        Ok(self.state_store.get_last_settlement()?)
    }

    pub fn query_settlement_for_epoch(
        &self,
        epoch_id: u64,
    ) -> Result<Option<EpochSettlement>, SettlementError> {
        Ok(self.state_store.get_epoch_settlement(epoch_id)?)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettlementRoutingConfig {
    pub validator_fee_share_ppm: u64,
    pub staking_fee_share_ppm: u64,
    pub treasury_fee_share_ppm: u64,
    pub validator_emission_share_ppm: u64,
    pub staking_emission_share_ppm: u64,
    pub treasury_emission_share_ppm: u64,
}

impl Default for SettlementRoutingConfig {
    fn default() -> Self {
        Self {
            validator_fee_share_ppm: 700_000,
            staking_fee_share_ppm: 100_000,
            treasury_fee_share_ppm: 100_000,
            validator_emission_share_ppm: 600_000,
            staking_emission_share_ppm: 200_000,
            treasury_emission_share_ppm: 100_000,
        }
    }
}

impl SettlementRoutingConfig {
    pub fn validate(&self) -> Result<(), SettlementConfigurationError> {
        let fee_total = self
            .validator_fee_share_ppm
            .checked_add(self.staking_fee_share_ppm)
            .and_then(|value| value.checked_add(self.treasury_fee_share_ppm))
            .ok_or(SettlementConfigurationError::ShareOverflow)?;
        let emission_total = self
            .validator_emission_share_ppm
            .checked_add(self.staking_emission_share_ppm)
            .and_then(|value| value.checked_add(self.treasury_emission_share_ppm))
            .ok_or(SettlementConfigurationError::ShareOverflow)?;
        if fee_total > PPM_DENOMINATOR || emission_total > PPM_DENOMINATOR {
            return Err(SettlementConfigurationError::SharesExceedPpm);
        }
        Ok(())
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SettlementConfigurationError {
    #[error("share total overflow")]
    ShareOverflow,
    #[error("routing share total exceeds ppm denominator")]
    SharesExceedPpm,
}

#[derive(Debug, Error)]
pub enum SettlementError {
    #[error("settlement already exists for epoch {epoch_id}")]
    SettlementAlreadyExists { epoch_id: u64 },
    #[error("policy epoch mismatch: policy={policy_epoch}, settlement={settlement_epoch}")]
    PolicyEpochMismatch {
        policy_epoch: u64,
        settlement_epoch: u64,
    },
    #[error("accounting overflow")]
    AccountingOverflow,
    #[error("accounting underflow")]
    AccountingUnderflow,
    #[error("accounting invariant mismatch: spent={spent}, funded={funded}")]
    AccountingInvariantViolation { spent: u128, funded: u128 },
    #[error("health settlement storage error: {0}")]
    Storage(#[from] HealthStorageError),
}

#[derive(Debug, Clone, Copy)]
struct RoutedAmounts {
    validator: u128,
    staking: u128,
    treasury: u128,
    insurance: u128,
}

fn route_amount(
    total: u128,
    validator_share_ppm: u64,
    staking_share_ppm: u64,
    treasury_share_ppm: u64,
) -> Result<RoutedAmounts, SettlementError> {
    let validator = ppm_mul_floor(total, validator_share_ppm)?;
    let staking = ppm_mul_floor(total, staking_share_ppm)?;
    let treasury = ppm_mul_floor(total, treasury_share_ppm)?;
    let distributed = validator
        .checked_add(staking)
        .and_then(|value| value.checked_add(treasury))
        .ok_or(SettlementError::AccountingOverflow)?;
    let insurance = total
        .checked_sub(distributed)
        .ok_or(SettlementError::AccountingUnderflow)?;
    Ok(RoutedAmounts {
        validator,
        staking,
        treasury,
        insurance,
    })
}

fn ppm_mul_floor(amount: u128, ppm: u64) -> Result<u128, SettlementError> {
    let mul = amount
        .checked_mul(ppm as u128)
        .ok_or(SettlementError::AccountingOverflow)?;
    Ok(mul / PPM_DENOMINATOR as u128)
}

fn settlement_hash(settlement: &EpochSettlement) -> [u8; HASH_LENGTH] {
    let mut bytes = Vec::with_capacity(8 + (8 * 16) + 1);
    bytes.extend_from_slice(&settlement.epoch_id.to_le_bytes());
    bytes.extend_from_slice(&settlement.total_fees.to_le_bytes());
    bytes.extend_from_slice(&settlement.burned_amount.to_le_bytes());
    bytes.extend_from_slice(&settlement.validator_reward_amount.to_le_bytes());
    bytes.extend_from_slice(&settlement.staking_reward_amount.to_le_bytes());
    bytes.extend_from_slice(&settlement.treasury_amount.to_le_bytes());
    bytes.extend_from_slice(&settlement.insurance_amount.to_le_bytes());
    bytes.extend_from_slice(&settlement.reserve_emission_amount.to_le_bytes());
    bytes.extend_from_slice(&settlement.minted_emission_amount.to_le_bytes());
    bytes.push(if settlement.hibernation_applied { 1 } else { 0 });
    let digest = Blake2b256::hash(bytes);
    let mut hash = [0u8; HASH_LENGTH];
    hash.copy_from_slice(&digest[..HASH_LENGTH]);
    hash
}

#[cfg(test)]
mod tests {
    use super::{
        EpochEconomicSettlement, SettlementConfigurationError, SettlementError,
        SettlementRoutingConfig,
    };
    use crate::rust::health::storage::HealthStateStore;
    use crate::rust::health::types::{EmissionMode, HealthRegime, PolicyState};

    fn settlement() -> EpochEconomicSettlement {
        EpochEconomicSettlement::new(
            SettlementRoutingConfig::default(),
            HealthStateStore::in_memory_for_tests(),
        )
        .expect("config should be valid")
    }

    fn policy(epoch_id: u64, emission_mode: EmissionMode) -> PolicyState {
        PolicyState {
            epoch_id,
            activation_epoch: epoch_id,
            source_snapshot_hash: [1; 32],
            health_score_ppm: 0,
            health_regime: HealthRegime::Healthy,
            burn_fraction_ppm: 100_000,
            reserve_emission_rate_ppm: 200_000,
            emission_mode,
            reserve_circuit_breaker: false,
            status_flags: 0,
        }
    }

    #[test]
    fn settle_epoch_economics_is_idempotent_by_epoch() {
        let settlement = settlement();
        let first =
            settlement.settle_epoch_economics(10, &policy(10, EmissionMode::Normal), 100, 50);
        assert!(first.is_ok());

        let second =
            settlement.settle_epoch_economics(10, &policy(10, EmissionMode::Normal), 100, 50);
        assert!(matches!(
            second,
            Err(SettlementError::SettlementAlreadyExists { epoch_id: 10 })
        ));
    }

    #[test]
    fn settle_epoch_economics_disables_emission_during_hibernation() {
        let settlement = settlement();
        let result =
            settlement.settle_epoch_economics(7, &policy(7, EmissionMode::Hibernating), 1000, 777);
        assert!(result.is_ok());

        let (record, _) = result.expect("expected settled record");
        assert_eq!(record.minted_emission_amount, 0);
        assert_eq!(record.reserve_emission_amount, 0);
        assert!(record.hibernation_applied);
    }

    #[test]
    fn settle_epoch_economics_preserves_accounting_identity() {
        let settlement = settlement();
        let (record, _) = settlement
            .settle_epoch_economics(12, &policy(12, EmissionMode::Normal), 100_000, 20_000)
            .expect("settlement should succeed");

        let spent = record.burned_amount
            + record.validator_reward_amount
            + record.staking_reward_amount
            + record.treasury_amount
            + record.insurance_amount
            + record.reserve_emission_amount;
        let funded = record.total_fees + record.minted_emission_amount;
        assert_eq!(spent, funded);
    }

    #[test]
    fn settle_epoch_economics_rejects_policy_epoch_mismatch() {
        let settlement = settlement();
        let result =
            settlement.settle_epoch_economics(8, &policy(7, EmissionMode::Normal), 1000, 100);
        assert!(matches!(
            result,
            Err(SettlementError::PolicyEpochMismatch {
                policy_epoch: 7,
                settlement_epoch: 8
            })
        ));
    }

    #[test]
    fn invalid_routing_config_is_rejected() {
        let config = SettlementRoutingConfig {
            validator_fee_share_ppm: 900_000,
            staking_fee_share_ppm: 200_000,
            treasury_fee_share_ppm: 100_000,
            ..SettlementRoutingConfig::default()
        };
        let result = EpochEconomicSettlement::new(config, HealthStateStore::in_memory_for_tests());
        assert!(matches!(
            result,
            Err(SettlementConfigurationError::SharesExceedPpm)
        ));
    }
}
