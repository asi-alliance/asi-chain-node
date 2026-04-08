use std::collections::BTreeMap;

use thiserror::Error;

use crate::rust::health::events::PolicyEvent;
use crate::rust::health::input_oracle::HealthMetricsPayloadV1;
use crate::rust::health::params::{PolicyParameterError, PolicyParameters};
use crate::rust::health::types::{
    EmissionMode, HealthInputSnapshot, HealthRegime, PolicyState, PPM_DENOMINATOR,
    STATUS_FLAG_FALLBACK_INPUT,
};

#[derive(Debug, Clone)]
pub struct HealthPolicyController {
    config: HealthPolicyControllerConfig,
    active_policy: Option<PolicyState>,
    policy_by_epoch: BTreeMap<u64, PolicyState>,
}

impl Default for HealthPolicyController {
    fn default() -> Self {
        Self {
            config: HealthPolicyControllerConfig::default(),
            active_policy: None,
            policy_by_epoch: BTreeMap::new(),
        }
    }
}

impl HealthPolicyController {
    pub fn new(
        config: HealthPolicyControllerConfig,
    ) -> Result<Self, PolicyControllerConfigurationError> {
        config.validate()?;
        Ok(Self {
            config,
            active_policy: None,
            policy_by_epoch: BTreeMap::new(),
        })
    }

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

        let previous_policy = self.active_policy.as_ref();
        let mut status_flags = 0u64;
        let (health_score_ppm, source_snapshot_hash) = match snapshot {
            Some(input_snapshot) => match self.score_from_snapshot(input_snapshot) {
                Some(score) => (score, input_snapshot.payload_hash),
                None => {
                    status_flags |= STATUS_FLAG_FALLBACK_INPUT;
                    self.fallback_score_and_hash(previous_policy)
                }
            },
            None => {
                status_flags |= STATUS_FLAG_FALLBACK_INPUT;
                self.fallback_score_and_hash(previous_policy)
            }
        };

        let mapped_regime = self.regime_for_score(health_score_ppm);
        let target_regime = self.apply_transition_guards(
            previous_policy,
            mapped_regime,
            health_score_ppm,
            target_epoch,
        );
        let profile = self.profile_for_regime(target_regime);

        let burn_target = self.clamp_to_policy_bounds(
            profile.burn_fraction_ppm,
            self.config.parameters.burn_fraction_min_ppm,
            self.config.parameters.burn_fraction_max_ppm,
        );
        let reserve_target = self.clamp_to_policy_bounds(
            profile.reserve_emission_rate_ppm,
            self.config.parameters.reserve_emission_min_ppm,
            self.config.parameters.reserve_emission_max_ppm,
        );

        let burn_fraction_ppm = previous_policy
            .map(|policy| {
                cap_delta(
                    policy.burn_fraction_ppm,
                    burn_target,
                    self.config.max_burn_change_ppm_per_epoch,
                )
            })
            .unwrap_or(burn_target);
        let reserve_emission_rate_ppm = previous_policy
            .map(|policy| {
                cap_delta(
                    policy.reserve_emission_rate_ppm,
                    reserve_target,
                    self.config.max_reserve_change_ppm_per_epoch,
                )
            })
            .unwrap_or(reserve_target);

        let policy = PolicyState {
            epoch_id: target_epoch,
            activation_epoch: target_epoch,
            source_snapshot_hash,
            health_score_ppm,
            health_regime: target_regime,
            burn_fraction_ppm,
            reserve_emission_rate_ppm,
            emission_mode: profile.emission_mode,
            reserve_circuit_breaker: profile.reserve_circuit_breaker,
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

    fn score_from_snapshot(&self, snapshot: &HealthInputSnapshot) -> Option<u64> {
        if snapshot.metrics_version != 1 {
            return None;
        }
        let payload: HealthMetricsPayloadV1 = bincode::deserialize(&snapshot.payload_bytes).ok()?;
        let average = ((payload.psi_ppm as u128 + payload.fee_quality_ppm as u128) / 2) as u64;
        Some(average.min(PPM_DENOMINATOR))
    }

    fn fallback_score_and_hash(&self, previous_policy: Option<&PolicyState>) -> (u64, [u8; 32]) {
        match previous_policy {
            Some(policy) => (policy.health_score_ppm, [0; 32]),
            None => (self.config.default_health_score_ppm, [0; 32]),
        }
    }

    fn regime_for_score(&self, health_score_ppm: u64) -> HealthRegime {
        if health_score_ppm <= self.config.thresholds.hibernation_max_score_ppm {
            HealthRegime::Hibernation
        } else if health_score_ppm <= self.config.thresholds.critical_max_score_ppm {
            HealthRegime::Critical
        } else if health_score_ppm <= self.config.thresholds.stressed_max_score_ppm {
            HealthRegime::Stressed
        } else if health_score_ppm <= self.config.thresholds.cautious_max_score_ppm {
            HealthRegime::Cautious
        } else {
            HealthRegime::Healthy
        }
    }

    fn apply_transition_guards(
        &self,
        previous_policy: Option<&PolicyState>,
        candidate_regime: HealthRegime,
        health_score_ppm: u64,
        target_epoch: u64,
    ) -> HealthRegime {
        let Some(previous) = previous_policy else {
            return candidate_regime;
        };

        let previous_regime = previous.health_regime;
        let previous_severity = regime_severity(previous_regime);
        let candidate_severity = regime_severity(candidate_regime);

        if target_epoch
            < previous
                .activation_epoch
                .saturating_add(self.config.parameters.min_regime_dwell_epochs)
        {
            return previous_regime;
        }

        if candidate_severity > previous_severity {
            if let Some(threshold) = self.entry_threshold(candidate_regime) {
                let required_score = threshold
                    .saturating_sub(self.config.parameters.upward_transition_hysteresis_ppm);
                if health_score_ppm > required_score {
                    return previous_regime;
                }
            }
        } else if candidate_severity < previous_severity {
            if let Some(previous_threshold) = self.entry_threshold(previous_regime) {
                let required_score = previous_threshold
                    .saturating_add(self.config.parameters.downward_transition_hysteresis_ppm)
                    .min(PPM_DENOMINATOR);
                if health_score_ppm < required_score {
                    return previous_regime;
                }
            }
        }

        candidate_regime
    }

    fn entry_threshold(&self, regime: HealthRegime) -> Option<u64> {
        match regime {
            HealthRegime::Healthy => None,
            HealthRegime::Cautious => Some(self.config.thresholds.cautious_max_score_ppm),
            HealthRegime::Stressed => Some(self.config.thresholds.stressed_max_score_ppm),
            HealthRegime::Critical => Some(self.config.thresholds.critical_max_score_ppm),
            HealthRegime::Hibernation => Some(self.config.thresholds.hibernation_max_score_ppm),
        }
    }

    fn profile_for_regime(&self, regime: HealthRegime) -> RegimePolicyProfile {
        match regime {
            HealthRegime::Healthy => self.config.profiles.healthy,
            HealthRegime::Cautious => self.config.profiles.cautious,
            HealthRegime::Stressed => self.config.profiles.stressed,
            HealthRegime::Critical => self.config.profiles.critical,
            HealthRegime::Hibernation => self.config.profiles.hibernation,
        }
    }

    fn clamp_to_policy_bounds(&self, value: u64, min: u64, max: u64) -> u64 {
        value.max(min).min(max)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HealthRegimeThresholds {
    pub cautious_max_score_ppm: u64,
    pub stressed_max_score_ppm: u64,
    pub critical_max_score_ppm: u64,
    pub hibernation_max_score_ppm: u64,
}

impl Default for HealthRegimeThresholds {
    fn default() -> Self {
        Self {
            cautious_max_score_ppm: 850_000,
            stressed_max_score_ppm: 650_000,
            critical_max_score_ppm: 400_000,
            hibernation_max_score_ppm: 200_000,
        }
    }
}

impl HealthRegimeThresholds {
    fn validate(&self) -> Result<(), PolicyControllerConfigurationError> {
        if self.hibernation_max_score_ppm > self.critical_max_score_ppm
            || self.critical_max_score_ppm > self.stressed_max_score_ppm
            || self.stressed_max_score_ppm > self.cautious_max_score_ppm
            || self.cautious_max_score_ppm > PPM_DENOMINATOR
        {
            return Err(PolicyControllerConfigurationError::InvalidRegimeThresholds);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegimePolicyProfile {
    pub burn_fraction_ppm: u64,
    pub reserve_emission_rate_ppm: u64,
    pub emission_mode: EmissionMode,
    pub reserve_circuit_breaker: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HealthRegimeProfiles {
    pub healthy: RegimePolicyProfile,
    pub cautious: RegimePolicyProfile,
    pub stressed: RegimePolicyProfile,
    pub critical: RegimePolicyProfile,
    pub hibernation: RegimePolicyProfile,
}

impl Default for HealthRegimeProfiles {
    fn default() -> Self {
        Self {
            healthy: RegimePolicyProfile {
                burn_fraction_ppm: 100_000,
                reserve_emission_rate_ppm: 180_000,
                emission_mode: EmissionMode::Normal,
                reserve_circuit_breaker: false,
            },
            cautious: RegimePolicyProfile {
                burn_fraction_ppm: 180_000,
                reserve_emission_rate_ppm: 140_000,
                emission_mode: EmissionMode::Normal,
                reserve_circuit_breaker: false,
            },
            stressed: RegimePolicyProfile {
                burn_fraction_ppm: 300_000,
                reserve_emission_rate_ppm: 80_000,
                emission_mode: EmissionMode::Normal,
                reserve_circuit_breaker: false,
            },
            critical: RegimePolicyProfile {
                burn_fraction_ppm: 500_000,
                reserve_emission_rate_ppm: 20_000,
                emission_mode: EmissionMode::Normal,
                reserve_circuit_breaker: true,
            },
            hibernation: RegimePolicyProfile {
                burn_fraction_ppm: 650_000,
                reserve_emission_rate_ppm: 0,
                emission_mode: EmissionMode::Hibernating,
                reserve_circuit_breaker: true,
            },
        }
    }
}

impl HealthRegimeProfiles {
    fn validate(&self) -> Result<(), PolicyControllerConfigurationError> {
        for profile in [
            self.healthy,
            self.cautious,
            self.stressed,
            self.critical,
            self.hibernation,
        ] {
            if profile.burn_fraction_ppm > PPM_DENOMINATOR
                || profile.reserve_emission_rate_ppm > PPM_DENOMINATOR
            {
                return Err(PolicyControllerConfigurationError::InvalidProfileBounds);
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthPolicyControllerConfig {
    pub thresholds: HealthRegimeThresholds,
    pub parameters: PolicyParameters,
    pub profiles: HealthRegimeProfiles,
    pub max_burn_change_ppm_per_epoch: u64,
    pub max_reserve_change_ppm_per_epoch: u64,
    pub default_health_score_ppm: u64,
}

impl Default for HealthPolicyControllerConfig {
    fn default() -> Self {
        Self {
            thresholds: HealthRegimeThresholds::default(),
            parameters: PolicyParameters {
                burn_fraction_min_ppm: 50_000,
                burn_fraction_max_ppm: 700_000,
                reserve_emission_min_ppm: 0,
                reserve_emission_max_ppm: 400_000,
                min_regime_dwell_epochs: 1,
                upward_transition_hysteresis_ppm: 20_000,
                downward_transition_hysteresis_ppm: 30_000,
                allow_emergency_override: false,
            },
            profiles: HealthRegimeProfiles::default(),
            max_burn_change_ppm_per_epoch: 150_000,
            max_reserve_change_ppm_per_epoch: 150_000,
            default_health_score_ppm: PPM_DENOMINATOR,
        }
    }
}

impl HealthPolicyControllerConfig {
    pub fn validate(&self) -> Result<(), PolicyControllerConfigurationError> {
        self.thresholds.validate()?;
        self.parameters
            .validate()
            .map_err(PolicyControllerConfigurationError::InvalidPolicyParameters)?;
        self.profiles.validate()?;
        if self.max_burn_change_ppm_per_epoch > PPM_DENOMINATOR
            || self.max_reserve_change_ppm_per_epoch > PPM_DENOMINATOR
            || self.default_health_score_ppm > PPM_DENOMINATOR
        {
            return Err(PolicyControllerConfigurationError::InvalidDeltaBounds);
        }
        Ok(())
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PolicyControllerConfigurationError {
    #[error("regime thresholds are not monotonic")]
    InvalidRegimeThresholds,
    #[error("policy profile values are out of ppm bounds")]
    InvalidProfileBounds,
    #[error("invalid per-epoch delta or default score bounds")]
    InvalidDeltaBounds,
    #[error("invalid policy parameters: {0}")]
    InvalidPolicyParameters(#[from] PolicyParameterError),
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

fn cap_delta(previous: u64, target: u64, max_delta: u64) -> u64 {
    if previous == target || max_delta == PPM_DENOMINATOR {
        return target;
    }
    if previous < target {
        previous.saturating_add(max_delta).min(target)
    } else {
        previous.saturating_sub(max_delta).max(target)
    }
}

fn regime_severity(regime: HealthRegime) -> u8 {
    match regime {
        HealthRegime::Healthy => 0,
        HealthRegime::Cautious => 1,
        HealthRegime::Stressed => 2,
        HealthRegime::Critical => 3,
        HealthRegime::Hibernation => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        HealthPolicyController, HealthPolicyControllerConfig, PolicyControllerConfigurationError,
        PolicyControllerError,
    };
    use crate::rust::health::input_oracle::{encode_metrics_payload_v1, HealthMetricsPayloadV1};
    use crate::rust::health::types::{
        HealthInputSnapshot, HealthRegime, STATUS_FLAG_FALLBACK_INPUT,
    };

    fn snapshot(target_epoch: u64, psi_ppm: u64, fee_quality_ppm: u64) -> HealthInputSnapshot {
        let payload_bytes = encode_metrics_payload_v1(&HealthMetricsPayloadV1 {
            psi_ppm,
            fee_quality_ppm,
        })
        .expect("must encode payload");
        HealthInputSnapshot {
            target_epoch,
            submitted_at_millis: 1000,
            metrics_version: 1,
            payload_hash: [9; 32],
            payload_bytes,
            signer_set_id: 1,
            quorum_verified: true,
            accepted: true,
        }
    }

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

    #[test]
    fn policy_controller_maps_score_to_expected_regimes() {
        let mut controller = HealthPolicyController::default();
        controller
            .activate_policy_for_epoch(10, 11, Some(&snapshot(11, 920_000, 920_000)))
            .expect("must activate healthy");
        assert_eq!(
            controller.query_current_policy().unwrap().health_regime,
            HealthRegime::Healthy
        );

        controller
            .activate_policy_for_epoch(11, 12, Some(&snapshot(12, 800_000, 800_000)))
            .expect("must activate cautious");
        assert_eq!(
            controller.query_current_policy().unwrap().health_regime,
            HealthRegime::Cautious
        );

        controller
            .activate_policy_for_epoch(12, 13, Some(&snapshot(13, 350_000, 350_000)))
            .expect("must activate critical");
        assert_eq!(
            controller.query_current_policy().unwrap().health_regime,
            HealthRegime::Critical
        );

        controller
            .activate_policy_for_epoch(13, 14, Some(&snapshot(14, 100_000, 100_000)))
            .expect("must activate hibernation");
        assert_eq!(
            controller.query_current_policy().unwrap().health_regime,
            HealthRegime::Hibernation
        );
    }

    #[test]
    fn hysteresis_prevents_immediate_flip_flop() {
        let mut controller = HealthPolicyController::default();
        controller
            .activate_policy_for_epoch(10, 11, Some(&snapshot(11, 920_000, 920_000)))
            .expect("must activate healthy");
        controller
            .activate_policy_for_epoch(11, 12, Some(&snapshot(12, 840_000, 840_000)))
            .expect("must keep healthy due upward hysteresis");
        assert_eq!(
            controller.query_current_policy().unwrap().health_regime,
            HealthRegime::Healthy
        );

        controller
            .activate_policy_for_epoch(12, 13, Some(&snapshot(13, 820_000, 820_000)))
            .expect("must move to cautious once threshold + hysteresis is crossed");
        assert_eq!(
            controller.query_current_policy().unwrap().health_regime,
            HealthRegime::Cautious
        );

        controller
            .activate_policy_for_epoch(13, 14, Some(&snapshot(14, 870_000, 870_000)))
            .expect("must remain cautious due downward hysteresis");
        assert_eq!(
            controller.query_current_policy().unwrap().health_regime,
            HealthRegime::Cautious
        );
    }

    #[test]
    fn invalid_config_is_rejected() {
        let mut bad_config = HealthPolicyControllerConfig::default();
        bad_config.thresholds.cautious_max_score_ppm = 100_000;
        bad_config.thresholds.stressed_max_score_ppm = 200_000;

        let result = HealthPolicyController::new(bad_config);
        assert!(matches!(
            result,
            Err(PolicyControllerConfigurationError::InvalidRegimeThresholds)
        ));
    }
}
