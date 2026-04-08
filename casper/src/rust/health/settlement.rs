use std::collections::BTreeMap;

use thiserror::Error;

use crate::rust::health::events::PolicyEvent;
use crate::rust::health::types::{EmissionMode, EpochSettlement, PolicyState, PPM_DENOMINATOR};

#[derive(Debug, Default, Clone)]
pub struct EpochEconomicSettlement {
    settlement_by_epoch: BTreeMap<u64, EpochSettlement>,
}

impl EpochEconomicSettlement {
    pub fn settle_epoch_economics(
        &mut self,
        epoch_id: u64,
        policy: &PolicyState,
        total_fees: u128,
        scheduled_emission: u128,
    ) -> Result<(EpochSettlement, PolicyEvent), SettlementError> {
        if self.settlement_by_epoch.contains_key(&epoch_id) {
            return Err(SettlementError::SettlementAlreadyExists { epoch_id });
        }
        if policy.epoch_id != epoch_id {
            return Err(SettlementError::PolicyEpochMismatch {
                policy_epoch: policy.epoch_id,
                settlement_epoch: epoch_id,
            });
        }

        let burned_amount = ppm_mul_floor(total_fees, policy.burn_fraction_ppm)?;
        let validator_reward_amount = total_fees
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
        let staking_reward_amount = minted_emission_amount
            .checked_sub(reserve_emission_amount)
            .ok_or(SettlementError::AccountingUnderflow)?;

        let settlement = EpochSettlement {
            epoch_id,
            total_fees,
            burned_amount,
            validator_reward_amount,
            staking_reward_amount,
            treasury_amount: 0,
            insurance_amount: 0,
            reserve_emission_amount,
            minted_emission_amount,
            hibernation_applied,
            settlement_hash: [0; 32],
        };

        self.settlement_by_epoch
            .insert(epoch_id, settlement.clone());
        let event = PolicyEvent::EpochSettled {
            epoch_id,
            burned_amount,
            minted_emission_amount,
            hibernation_applied,
        };

        Ok((settlement, event))
    }

    pub fn query_last_settlement(&self) -> Option<&EpochSettlement> {
        self.settlement_by_epoch.last_key_value().map(|(_, v)| v)
    }

    pub fn query_settlement_for_epoch(&self, epoch_id: u64) -> Option<&EpochSettlement> {
        self.settlement_by_epoch.get(&epoch_id)
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
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
}

fn ppm_mul_floor(amount: u128, ppm: u64) -> Result<u128, SettlementError> {
    let mul = amount
        .checked_mul(ppm as u128)
        .ok_or(SettlementError::AccountingOverflow)?;
    Ok(mul / PPM_DENOMINATOR as u128)
}

#[cfg(test)]
mod tests {
    use super::{EpochEconomicSettlement, SettlementError};
    use crate::rust::health::types::{EmissionMode, HealthRegime, PolicyState};

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
        let mut settlement = EpochEconomicSettlement::default();
        let first =
            settlement.settle_epoch_economics(10, &policy(10, EmissionMode::Normal), 100, 50);
        assert!(first.is_ok());

        let second =
            settlement.settle_epoch_economics(10, &policy(10, EmissionMode::Normal), 100, 50);
        assert_eq!(
            second,
            Err(SettlementError::SettlementAlreadyExists { epoch_id: 10 })
        );
    }

    #[test]
    fn settle_epoch_economics_disables_emission_during_hibernation() {
        let mut settlement = EpochEconomicSettlement::default();
        let result =
            settlement.settle_epoch_economics(7, &policy(7, EmissionMode::Hibernating), 1000, 777);
        assert!(result.is_ok());

        let (record, _) = result.expect("expected settled record");
        assert_eq!(record.minted_emission_amount, 0);
        assert_eq!(record.reserve_emission_amount, 0);
        assert!(record.hibernation_applied);
    }
}
