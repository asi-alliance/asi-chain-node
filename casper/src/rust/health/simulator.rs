use crypto::rust::hash::blake2b256::Blake2b256;
use thiserror::Error;

use crate::rust::health::control_layer::{
    HealthControlConfigurationError, HealthControlError, HealthControlLayer,
    HealthControlLayerConfig,
};
use crate::rust::health::events::PolicyEvent;
use crate::rust::health::input_oracle::{
    encode_metrics_payload_v1, HealthInputBundle, HealthInputSignature, HealthMetricsPayloadV1,
};
use crate::rust::health::storage::HealthStateStore;
use crate::rust::health::types::{EmissionMode, EpochSettlement, HealthRegime};

#[derive(Clone)]
pub struct HealthShadowSimulator {
    control_layer: HealthControlLayer,
}

impl HealthShadowSimulator {
    pub fn new_default() -> Result<Self, HealthControlConfigurationError> {
        let control_layer = HealthControlLayer::new(
            HealthControlLayerConfig::default(),
            HealthStateStore::in_memory(),
        )?;
        Ok(Self { control_layer })
    }

    pub fn new(config: HealthControlLayerConfig) -> Result<Self, HealthControlConfigurationError> {
        let control_layer = HealthControlLayer::new(config, HealthStateStore::in_memory())?;
        Ok(Self { control_layer })
    }

    pub fn run(
        &mut self,
        epoch_inputs: &[ShadowEpochInput],
    ) -> Result<ShadowSimulationReport, ShadowSimulationError> {
        let mut outputs = Vec::with_capacity(epoch_inputs.len());
        let mut fallback_epochs = Vec::new();

        for input in epoch_inputs {
            let current_epoch = input.epoch_id.checked_sub(1).ok_or(
                ShadowSimulationError::InvalidEpochSequence {
                    epoch_id: input.epoch_id,
                },
            )?;

            if let Some(metrics) = input.metrics {
                let bundle = shadow_bundle(input.epoch_id, metrics)?;
                self.control_layer
                    .submit_health_input(current_epoch, bundle)?;
            }

            self.control_layer
                .activate_policy_for_epoch(current_epoch, input.epoch_id)?;
            let policy = self.control_layer.query_current_policy()?.ok_or(
                ShadowSimulationError::MissingPolicyAfterActivation {
                    epoch_id: input.epoch_id,
                },
            )?;

            let (settlement, _) = self.control_layer.settle_epoch_economics(
                input.epoch_id,
                input.total_fees,
                input.scheduled_emission,
            )?;

            let used_fallback =
                (policy.status_flags & crate::rust::health::types::STATUS_FLAG_FALLBACK_INPUT) != 0;
            if used_fallback {
                fallback_epochs.push(input.epoch_id);
            }

            outputs.push(ShadowEpochOutput {
                epoch_id: input.epoch_id,
                health_regime: policy.health_regime,
                burn_fraction_ppm: policy.burn_fraction_ppm,
                reserve_emission_rate_ppm: policy.reserve_emission_rate_ppm,
                emission_mode: policy.emission_mode,
                used_fallback,
                settlement,
            });
        }

        Ok(ShadowSimulationReport {
            outputs,
            event_count: self.control_layer.events().len(),
            fallback_epochs,
            events: self.control_layer.events().to_vec(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShadowEpochInput {
    pub epoch_id: u64,
    pub metrics: Option<HealthMetricsPayloadV1>,
    pub total_fees: u128,
    pub scheduled_emission: u128,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShadowEpochOutput {
    pub epoch_id: u64,
    pub health_regime: HealthRegime,
    pub burn_fraction_ppm: u64,
    pub reserve_emission_rate_ppm: u64,
    pub emission_mode: EmissionMode,
    pub used_fallback: bool,
    pub settlement: EpochSettlement,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShadowSimulationReport {
    pub outputs: Vec<ShadowEpochOutput>,
    pub event_count: usize,
    pub fallback_epochs: Vec<u64>,
    pub events: Vec<PolicyEvent>,
}

impl ShadowSimulationReport {
    pub fn compare_regimes(&self, expected: &[ExpectedRegime]) -> ShadowRegimeComparison {
        let mut mismatches = Vec::new();
        for expected_regime in expected {
            let actual = self
                .outputs
                .iter()
                .find(|item| item.epoch_id == expected_regime.epoch_id)
                .map(|item| item.health_regime);
            if actual != Some(expected_regime.regime) {
                mismatches.push(ShadowRegimeMismatch {
                    epoch_id: expected_regime.epoch_id,
                    expected: expected_regime.regime,
                    actual,
                });
            }
        }
        ShadowRegimeComparison {
            matched: mismatches.is_empty(),
            mismatches,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExpectedRegime {
    pub epoch_id: u64,
    pub regime: HealthRegime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShadowRegimeComparison {
    pub matched: bool,
    pub mismatches: Vec<ShadowRegimeMismatch>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShadowRegimeMismatch {
    pub epoch_id: u64,
    pub expected: HealthRegime,
    pub actual: Option<HealthRegime>,
}

#[derive(Debug, Error)]
pub enum ShadowSimulationError {
    #[error("invalid epoch sequence input: epoch_id={epoch_id}")]
    InvalidEpochSequence { epoch_id: u64 },
    #[error("policy missing after activation for epoch {epoch_id}")]
    MissingPolicyAfterActivation { epoch_id: u64 },
    #[error("control-layer error: {0}")]
    Control(#[from] HealthControlError),
    #[error("metrics encoding failed: {0}")]
    Encode(#[from] bincode::Error),
}

fn shadow_bundle(
    target_epoch: u64,
    metrics: HealthMetricsPayloadV1,
) -> Result<HealthInputBundle, bincode::Error> {
    let payload_bytes = encode_metrics_payload_v1(&metrics)?;
    let payload_hash = {
        let digest = Blake2b256::hash(payload_bytes.clone());
        let mut out = [0u8; 32];
        out.copy_from_slice(&digest[..32]);
        out
    };
    Ok(HealthInputBundle {
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
    })
}

pub fn scenario_healthy_baseline(start_epoch: u64) -> Vec<ShadowEpochInput> {
    vec![
        ShadowEpochInput {
            epoch_id: start_epoch,
            metrics: Some(HealthMetricsPayloadV1 {
                psi_ppm: 930_000,
                fee_quality_ppm: 920_000,
            }),
            total_fees: 120_000,
            scheduled_emission: 10_000,
        },
        ShadowEpochInput {
            epoch_id: start_epoch + 1,
            metrics: Some(HealthMetricsPayloadV1 {
                psi_ppm: 920_000,
                fee_quality_ppm: 910_000,
            }),
            total_fees: 118_000,
            scheduled_emission: 10_000,
        },
    ]
}

pub fn scenario_moderate_deterioration(start_epoch: u64) -> Vec<ShadowEpochInput> {
    vec![
        ShadowEpochInput {
            epoch_id: start_epoch,
            metrics: Some(HealthMetricsPayloadV1 {
                psi_ppm: 900_000,
                fee_quality_ppm: 900_000,
            }),
            total_fees: 100_000,
            scheduled_emission: 10_000,
        },
        ShadowEpochInput {
            epoch_id: start_epoch + 1,
            metrics: Some(HealthMetricsPayloadV1 {
                psi_ppm: 820_000,
                fee_quality_ppm: 820_000,
            }),
            total_fees: 95_000,
            scheduled_emission: 10_000,
        },
        ShadowEpochInput {
            epoch_id: start_epoch + 2,
            metrics: Some(HealthMetricsPayloadV1 {
                psi_ppm: 610_000,
                fee_quality_ppm: 620_000,
            }),
            total_fees: 90_000,
            scheduled_emission: 9_000,
        },
    ]
}

pub fn scenario_critical_stress(start_epoch: u64) -> Vec<ShadowEpochInput> {
    vec![
        ShadowEpochInput {
            epoch_id: start_epoch,
            metrics: Some(HealthMetricsPayloadV1 {
                psi_ppm: 780_000,
                fee_quality_ppm: 790_000,
            }),
            total_fees: 80_000,
            scheduled_emission: 8_000,
        },
        ShadowEpochInput {
            epoch_id: start_epoch + 1,
            metrics: Some(HealthMetricsPayloadV1 {
                psi_ppm: 320_000,
                fee_quality_ppm: 300_000,
            }),
            total_fees: 70_000,
            scheduled_emission: 8_000,
        },
        ShadowEpochInput {
            epoch_id: start_epoch + 2,
            metrics: Some(HealthMetricsPayloadV1 {
                psi_ppm: 120_000,
                fee_quality_ppm: 100_000,
            }),
            total_fees: 60_000,
            scheduled_emission: 8_000,
        },
    ]
}

pub fn scenario_hibernation_recovery(start_epoch: u64) -> Vec<ShadowEpochInput> {
    vec![
        ShadowEpochInput {
            epoch_id: start_epoch,
            metrics: Some(HealthMetricsPayloadV1 {
                psi_ppm: 110_000,
                fee_quality_ppm: 120_000,
            }),
            total_fees: 50_000,
            scheduled_emission: 8_000,
        },
        ShadowEpochInput {
            epoch_id: start_epoch + 1,
            metrics: Some(HealthMetricsPayloadV1 {
                psi_ppm: 430_000,
                fee_quality_ppm: 420_000,
            }),
            total_fees: 65_000,
            scheduled_emission: 8_000,
        },
        ShadowEpochInput {
            epoch_id: start_epoch + 2,
            metrics: Some(HealthMetricsPayloadV1 {
                psi_ppm: 700_000,
                fee_quality_ppm: 710_000,
            }),
            total_fees: 80_000,
            scheduled_emission: 8_000,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::{
        scenario_healthy_baseline, scenario_hibernation_recovery, scenario_moderate_deterioration,
        ExpectedRegime, HealthShadowSimulator, ShadowSimulationReport,
    };
    use crate::rust::health::types::HealthRegime;

    #[test]
    fn healthy_baseline_scenario_stays_healthy() {
        let mut simulator = HealthShadowSimulator::new_default().expect("must build simulator");
        let report = simulator
            .run(&scenario_healthy_baseline(20))
            .expect("simulation should succeed");
        assert!(report
            .outputs
            .iter()
            .all(|output| output.health_regime == HealthRegime::Healthy));
    }

    #[test]
    fn moderate_deterioration_scenario_transitions_to_stressed() {
        let mut simulator = HealthShadowSimulator::new_default().expect("must build simulator");
        let report = simulator
            .run(&scenario_moderate_deterioration(30))
            .expect("simulation should succeed");
        let regimes: Vec<_> = report
            .outputs
            .iter()
            .map(|output| output.health_regime)
            .collect();
        assert_eq!(
            regimes,
            vec![
                HealthRegime::Healthy,
                HealthRegime::Cautious,
                HealthRegime::Stressed
            ]
        );
    }

    #[test]
    fn hibernation_recovery_scenario_recovers_stepwise() {
        let mut simulator = HealthShadowSimulator::new_default().expect("must build simulator");
        let report = simulator
            .run(&scenario_hibernation_recovery(40))
            .expect("simulation should succeed");
        let regimes: Vec<_> = report
            .outputs
            .iter()
            .map(|output| output.health_regime)
            .collect();
        assert_eq!(
            regimes,
            vec![
                HealthRegime::Hibernation,
                HealthRegime::Stressed,
                HealthRegime::Cautious
            ]
        );
    }

    #[test]
    fn regime_comparison_reports_mismatches() {
        let report = ShadowSimulationReport {
            outputs: vec![super::ShadowEpochOutput {
                epoch_id: 7,
                health_regime: HealthRegime::Healthy,
                burn_fraction_ppm: 0,
                reserve_emission_rate_ppm: 0,
                emission_mode: crate::rust::health::types::EmissionMode::Normal,
                used_fallback: false,
                settlement: crate::rust::health::types::EpochSettlement {
                    epoch_id: 7,
                    total_fees: 0,
                    burned_amount: 0,
                    validator_reward_amount: 0,
                    staking_reward_amount: 0,
                    treasury_amount: 0,
                    insurance_amount: 0,
                    reserve_emission_amount: 0,
                    minted_emission_amount: 0,
                    hibernation_applied: false,
                    settlement_hash: [0; 32],
                },
            }],
            event_count: 0,
            fallback_epochs: vec![],
            events: vec![],
        };

        let comparison = report.compare_regimes(&[ExpectedRegime {
            epoch_id: 7,
            regime: HealthRegime::Critical,
        }]);
        assert!(!comparison.matched);
        assert_eq!(comparison.mismatches.len(), 1);
    }
}
