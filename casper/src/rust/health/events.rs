use serde::{Deserialize, Serialize};

use crate::rust::health::types::{Hash32, HealthRegime};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthInputRejectionReason {
    Stale,
    InvalidSchema,
    OutOfBounds,
    InsufficientQuorum,
    InvalidTargetEpoch,
    DuplicateSubmission,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PolicyEvent {
    HealthInputAccepted {
        target_epoch: u64,
        payload_hash: Hash32,
    },
    HealthInputRejected {
        target_epoch: u64,
        reason: HealthInputRejectionReason,
    },
    PolicyActivated {
        epoch_id: u64,
        regime: HealthRegime,
        source_snapshot_hash: Hash32,
    },
    EpochSettled {
        epoch_id: u64,
        burned_amount: u128,
        minted_emission_amount: u128,
        hibernation_applied: bool,
    },
}
