use std::collections::BTreeSet;

use crypto::rust::hash::blake2b256::{Blake2b256, HASH_LENGTH};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::rust::health::events::{HealthInputRejectionReason, PolicyEvent};
use crate::rust::health::storage::{HealthStateStore, HealthStorageError};
use crate::rust::health::types::HealthInputSnapshot;

#[derive(Clone)]
pub struct HealthInputOracle {
    config: HealthInputOracleConfig,
    state_store: HealthStateStore,
}

impl HealthInputOracle {
    pub fn new(
        config: HealthInputOracleConfig,
        state_store: HealthStateStore,
    ) -> Result<Self, HealthInputConfigurationError> {
        config.validate()?;
        Ok(Self {
            config,
            state_store,
        })
    }

    pub fn submit_health_input(
        &self,
        current_epoch: u64,
        bundle: HealthInputBundle,
    ) -> Result<PolicyEvent, HealthInputError> {
        self.validate_target_epoch(current_epoch, bundle.target_epoch)?;
        self.validate_bundle_shape(&bundle)?;
        let payload = decode_metrics_payload_v1(&bundle.payload_bytes).map_err(|_| {
            rejected(
                bundle.target_epoch,
                HealthInputRejectionReason::InvalidSchema,
            )
        })?;
        self.validate_metric_bounds(bundle.target_epoch, &payload)?;
        self.validate_signatures(&bundle)?;

        if self
            .state_store
            .get_pending_snapshot(bundle.target_epoch)?
            .is_some()
        {
            return Err(rejected(
                bundle.target_epoch,
                HealthInputRejectionReason::DuplicateSubmission,
            ));
        }

        let snapshot = HealthInputSnapshot {
            target_epoch: bundle.target_epoch,
            submitted_at_millis: bundle.submitted_at_millis,
            metrics_version: bundle.metrics_version,
            payload_hash: bundle.payload_hash,
            payload_bytes: bundle.payload_bytes,
            signer_set_id: bundle.signer_set_id,
            quorum_verified: true,
            accepted: true,
        };

        self.state_store.put_pending_snapshot(&snapshot)?;

        Ok(PolicyEvent::HealthInputAccepted {
            target_epoch: snapshot.target_epoch,
            payload_hash: snapshot.payload_hash,
        })
    }

    pub fn query_pending_snapshot(
        &self,
        target_epoch: u64,
    ) -> Result<Option<HealthInputSnapshot>, HealthInputError> {
        Ok(self.state_store.get_pending_snapshot(target_epoch)?)
    }

    pub fn query_latest_pending_snapshot(
        &self,
    ) -> Result<Option<HealthInputSnapshot>, HealthInputError> {
        Ok(self.state_store.get_latest_pending_snapshot()?)
    }

    pub fn consume_pending_snapshot(
        &self,
        target_epoch: u64,
    ) -> Result<Option<HealthInputSnapshot>, HealthInputError> {
        let snapshot = self.state_store.get_pending_snapshot(target_epoch)?;
        if snapshot.is_some() {
            self.state_store.delete_pending_snapshot(target_epoch)?;
        }
        Ok(snapshot)
    }

    fn validate_target_epoch(
        &self,
        current_epoch: u64,
        target_epoch: u64,
    ) -> Result<(), HealthInputError> {
        if target_epoch <= current_epoch {
            return Err(rejected(
                target_epoch,
                HealthInputRejectionReason::InvalidTargetEpoch,
            ));
        }
        let delta = target_epoch - current_epoch;
        if delta < self.config.min_target_epoch_delta || delta > self.config.max_target_epoch_delta
        {
            return Err(rejected(
                target_epoch,
                HealthInputRejectionReason::InvalidTargetEpoch,
            ));
        }
        Ok(())
    }

    fn validate_bundle_shape(&self, bundle: &HealthInputBundle) -> Result<(), HealthInputError> {
        if !self
            .config
            .supported_metrics_versions
            .contains(&bundle.metrics_version)
        {
            return Err(rejected(
                bundle.target_epoch,
                HealthInputRejectionReason::InvalidSchema,
            ));
        }
        if bundle.payload_bytes.is_empty()
            || bundle.payload_bytes.len() > self.config.max_payload_bytes
        {
            return Err(rejected(
                bundle.target_epoch,
                HealthInputRejectionReason::InvalidSchema,
            ));
        }

        let computed_hash = hash_payload(&bundle.payload_bytes);
        if computed_hash != bundle.payload_hash {
            return Err(rejected(
                bundle.target_epoch,
                HealthInputRejectionReason::InvalidSchema,
            ));
        }
        Ok(())
    }

    fn validate_metric_bounds(
        &self,
        target_epoch: u64,
        payload: &HealthMetricsPayloadV1,
    ) -> Result<(), HealthInputError> {
        if !self.config.psi_ppm_bounds.contains(payload.psi_ppm)
            || !self
                .config
                .fee_quality_ppm_bounds
                .contains(payload.fee_quality_ppm)
        {
            return Err(rejected(
                target_epoch,
                HealthInputRejectionReason::OutOfBounds,
            ));
        }
        Ok(())
    }

    fn validate_signatures(&self, bundle: &HealthInputBundle) -> Result<(), HealthInputError> {
        if bundle.signer_set_id != self.config.signer_set_id {
            return Err(rejected(
                bundle.target_epoch,
                HealthInputRejectionReason::InsufficientQuorum,
            ));
        }

        let mut unique_signers = BTreeSet::new();
        for signature in &bundle.signatures {
            if signature.sig_bytes.is_empty()
                || !self.config.authorized_signers.contains(&signature.signer)
            {
                return Err(rejected(
                    bundle.target_epoch,
                    HealthInputRejectionReason::InsufficientQuorum,
                ));
            }
            unique_signers.insert(signature.signer.clone());
        }

        if unique_signers.len() < self.config.quorum_threshold {
            return Err(rejected(
                bundle.target_epoch,
                HealthInputRejectionReason::InsufficientQuorum,
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthInputBundle {
    pub target_epoch: u64,
    pub submitted_at_millis: u64,
    pub metrics_version: u32,
    pub payload_hash: [u8; HASH_LENGTH],
    pub payload_bytes: Vec<u8>,
    pub signer_set_id: u32,
    pub signatures: Vec<HealthInputSignature>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthInputSignature {
    pub signer: String,
    pub sig_bytes: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthMetricsPayloadV1 {
    pub psi_ppm: u64,
    pub fee_quality_ppm: u64,
}

pub fn encode_metrics_payload_v1(
    payload: &HealthMetricsPayloadV1,
) -> Result<Vec<u8>, bincode::Error> {
    bincode::serialize(payload)
}

fn decode_metrics_payload_v1(bytes: &[u8]) -> Result<HealthMetricsPayloadV1, bincode::Error> {
    bincode::deserialize(bytes)
}

fn hash_payload(payload_bytes: &[u8]) -> [u8; HASH_LENGTH] {
    let digest = Blake2b256::hash(payload_bytes.to_vec());
    let mut out = [0u8; HASH_LENGTH];
    out.copy_from_slice(&digest[..HASH_LENGTH]);
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MetricBounds {
    pub min: u64,
    pub max: u64,
}

impl MetricBounds {
    fn contains(&self, value: u64) -> bool {
        value >= self.min && value <= self.max
    }

    fn validate(&self) -> Result<(), HealthInputConfigurationError> {
        if self.min > self.max {
            return Err(HealthInputConfigurationError::InvalidMetricBounds {
                min: self.min,
                max: self.max,
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthInputOracleConfig {
    pub min_target_epoch_delta: u64,
    pub max_target_epoch_delta: u64,
    pub max_payload_bytes: usize,
    pub supported_metrics_versions: BTreeSet<u32>,
    pub signer_set_id: u32,
    pub authorized_signers: BTreeSet<String>,
    pub quorum_threshold: usize,
    pub psi_ppm_bounds: MetricBounds,
    pub fee_quality_ppm_bounds: MetricBounds,
}

impl Default for HealthInputOracleConfig {
    fn default() -> Self {
        Self {
            min_target_epoch_delta: 1,
            max_target_epoch_delta: 64,
            max_payload_bytes: 32 * 1024,
            supported_metrics_versions: BTreeSet::from([1]),
            signer_set_id: 1,
            authorized_signers: BTreeSet::from([
                "oracle-1".to_string(),
                "oracle-2".to_string(),
                "oracle-3".to_string(),
            ]),
            quorum_threshold: 2,
            psi_ppm_bounds: MetricBounds {
                min: 0,
                max: 1_000_000,
            },
            fee_quality_ppm_bounds: MetricBounds {
                min: 0,
                max: 1_000_000,
            },
        }
    }
}

impl HealthInputOracleConfig {
    pub fn validate(&self) -> Result<(), HealthInputConfigurationError> {
        if self.min_target_epoch_delta == 0
            || self.min_target_epoch_delta > self.max_target_epoch_delta
        {
            return Err(HealthInputConfigurationError::InvalidTargetEpochDelta {
                min: self.min_target_epoch_delta,
                max: self.max_target_epoch_delta,
            });
        }
        if self.supported_metrics_versions.is_empty() {
            return Err(HealthInputConfigurationError::NoSupportedMetricVersion);
        }
        if self.authorized_signers.is_empty() {
            return Err(HealthInputConfigurationError::NoAuthorizedSigners);
        }
        if self.quorum_threshold == 0 || self.quorum_threshold > self.authorized_signers.len() {
            return Err(HealthInputConfigurationError::InvalidQuorumThreshold {
                quorum_threshold: self.quorum_threshold,
                authorized_signers: self.authorized_signers.len(),
            });
        }
        self.psi_ppm_bounds.validate()?;
        self.fee_quality_ppm_bounds.validate()?;
        Ok(())
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum HealthInputConfigurationError {
    #[error("invalid target epoch delta configuration: min={min}, max={max}")]
    InvalidTargetEpochDelta { min: u64, max: u64 },
    #[error("at least one supported metrics version is required")]
    NoSupportedMetricVersion,
    #[error("at least one authorized signer is required")]
    NoAuthorizedSigners,
    #[error(
        "invalid quorum threshold configuration: threshold={quorum_threshold}, signers={authorized_signers}"
    )]
    InvalidQuorumThreshold {
        quorum_threshold: usize,
        authorized_signers: usize,
    },
    #[error("invalid metric bounds: min={min}, max={max}")]
    InvalidMetricBounds { min: u64, max: u64 },
}

#[derive(Debug, Error)]
pub enum HealthInputError {
    #[error("health input rejected")]
    Rejected(PolicyEvent),
    #[error("health input storage error: {0}")]
    Storage(#[from] HealthStorageError),
}

fn rejected(target_epoch: u64, reason: HealthInputRejectionReason) -> HealthInputError {
    HealthInputError::Rejected(PolicyEvent::HealthInputRejected {
        target_epoch,
        reason,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        encode_metrics_payload_v1, hash_payload, HealthInputBundle, HealthInputError,
        HealthInputOracle, HealthInputOracleConfig, HealthInputSignature, HealthMetricsPayloadV1,
    };
    use crate::rust::health::events::{HealthInputRejectionReason, PolicyEvent};
    use crate::rust::health::storage::HealthStateStore;

    fn oracle() -> HealthInputOracle {
        HealthInputOracle::new(
            HealthInputOracleConfig::default(),
            HealthStateStore::in_memory_for_tests(),
        )
        .expect("oracle config should be valid")
    }

    fn bundle(
        target_epoch: u64,
        metrics: HealthMetricsPayloadV1,
        signer_names: &[&str],
    ) -> HealthInputBundle {
        let payload_bytes = encode_metrics_payload_v1(&metrics).expect("must encode");
        let payload_hash = hash_payload(&payload_bytes);
        HealthInputBundle {
            target_epoch,
            submitted_at_millis: 1000,
            metrics_version: 1,
            payload_hash,
            payload_bytes,
            signer_set_id: 1,
            signatures: signer_names
                .iter()
                .map(|name| HealthInputSignature {
                    signer: (*name).to_string(),
                    sig_bytes: vec![1, 2, 3],
                })
                .collect(),
        }
    }

    #[test]
    fn submit_health_input_accepts_valid_future_bundle() {
        let oracle = oracle();
        let bundle = bundle(
            11,
            HealthMetricsPayloadV1 {
                psi_ppm: 100_000,
                fee_quality_ppm: 900_000,
            },
            &["oracle-1", "oracle-2"],
        );
        let event = oracle
            .submit_health_input(10, bundle.clone())
            .expect("bundle should be accepted");

        assert_eq!(
            event,
            PolicyEvent::HealthInputAccepted {
                target_epoch: 11,
                payload_hash: bundle.payload_hash,
            }
        );
        let stored = oracle
            .query_pending_snapshot(11)
            .expect("query should succeed")
            .expect("snapshot should exist");
        assert_eq!(stored.target_epoch, 11);
    }

    #[test]
    fn submit_health_input_rejects_non_future_epoch() {
        let oracle = oracle();
        let result = oracle.submit_health_input(
            10,
            bundle(
                10,
                HealthMetricsPayloadV1 {
                    psi_ppm: 100_000,
                    fee_quality_ppm: 900_000,
                },
                &["oracle-1", "oracle-2"],
            ),
        );
        assert!(matches!(
            result,
            Err(HealthInputError::Rejected(
                PolicyEvent::HealthInputRejected {
                    target_epoch: 10,
                    reason: HealthInputRejectionReason::InvalidTargetEpoch
                }
            ))
        ));
    }

    #[test]
    fn submit_health_input_rejects_out_of_bounds_metric() {
        let oracle = oracle();
        let result = oracle.submit_health_input(
            10,
            bundle(
                11,
                HealthMetricsPayloadV1 {
                    psi_ppm: 1_500_000,
                    fee_quality_ppm: 900_000,
                },
                &["oracle-1", "oracle-2"],
            ),
        );
        assert!(matches!(
            result,
            Err(HealthInputError::Rejected(
                PolicyEvent::HealthInputRejected {
                    target_epoch: 11,
                    reason: HealthInputRejectionReason::OutOfBounds
                }
            ))
        ));
    }

    #[test]
    fn submit_health_input_rejects_insufficient_quorum() {
        let oracle = oracle();
        let result = oracle.submit_health_input(
            10,
            bundle(
                11,
                HealthMetricsPayloadV1 {
                    psi_ppm: 100_000,
                    fee_quality_ppm: 900_000,
                },
                &["oracle-1"],
            ),
        );
        assert!(matches!(
            result,
            Err(HealthInputError::Rejected(
                PolicyEvent::HealthInputRejected {
                    target_epoch: 11,
                    reason: HealthInputRejectionReason::InsufficientQuorum
                }
            ))
        ));
    }

    #[test]
    fn submit_health_input_rejects_duplicate_submission() {
        let oracle = oracle();
        let bundle = bundle(
            11,
            HealthMetricsPayloadV1 {
                psi_ppm: 100_000,
                fee_quality_ppm: 900_000,
            },
            &["oracle-1", "oracle-2"],
        );
        oracle
            .submit_health_input(10, bundle.clone())
            .expect("first submission should pass");

        let second = oracle.submit_health_input(10, bundle);
        assert!(matches!(
            second,
            Err(HealthInputError::Rejected(
                PolicyEvent::HealthInputRejected {
                    target_epoch: 11,
                    reason: HealthInputRejectionReason::DuplicateSubmission
                }
            ))
        ));
    }

    #[test]
    fn submit_health_input_rejects_invalid_schema_or_version() {
        let oracle = oracle();
        let mut bundle = bundle(
            11,
            HealthMetricsPayloadV1 {
                psi_ppm: 100_000,
                fee_quality_ppm: 900_000,
            },
            &["oracle-1", "oracle-2"],
        );
        bundle.metrics_version = 2;

        let result = oracle.submit_health_input(10, bundle);
        assert!(matches!(
            result,
            Err(HealthInputError::Rejected(
                PolicyEvent::HealthInputRejected {
                    target_epoch: 11,
                    reason: HealthInputRejectionReason::InvalidSchema
                }
            ))
        ));
    }
}
