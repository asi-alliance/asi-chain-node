use serde::{de::DeserializeOwned, Deserialize, Serialize};
use thiserror::Error;

use crate::rust::health::params::PolicyParameters;
use crate::rust::health::types::{EpochSettlement, HealthInputSnapshot, PolicyState};

pub const HEALTH_INPUT_SNAPSHOT_SCHEMA_VERSION: u16 = 1;
pub const POLICY_STATE_SCHEMA_VERSION: u16 = 1;
pub const EPOCH_SETTLEMENT_SCHEMA_VERSION: u16 = 1;
pub const POLICY_PARAMETERS_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum HealthCodecError {
    #[error("unsupported schema version for {artifact}: expected={expected}, actual={actual}")]
    UnsupportedSchemaVersion {
        artifact: &'static str,
        expected: u16,
        actual: u16,
    },
    #[error("serialization failure for {artifact}: {message}")]
    SerializationFailure {
        artifact: &'static str,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct VersionedEnvelope<T> {
    version: u16,
    payload: T,
}

#[derive(Debug, Serialize)]
struct VersionedEnvelopeRef<'a, T> {
    version: u16,
    payload: &'a T,
}

pub fn serialize_health_input_snapshot(
    value: &HealthInputSnapshot,
) -> Result<Vec<u8>, HealthCodecError> {
    encode_versioned(
        value,
        HEALTH_INPUT_SNAPSHOT_SCHEMA_VERSION,
        "HealthInputSnapshot",
    )
}

pub fn deserialize_health_input_snapshot(
    bytes: &[u8],
) -> Result<HealthInputSnapshot, HealthCodecError> {
    decode_versioned(
        bytes,
        HEALTH_INPUT_SNAPSHOT_SCHEMA_VERSION,
        "HealthInputSnapshot",
    )
}

pub fn serialize_policy_state(value: &PolicyState) -> Result<Vec<u8>, HealthCodecError> {
    encode_versioned(value, POLICY_STATE_SCHEMA_VERSION, "PolicyState")
}

pub fn deserialize_policy_state(bytes: &[u8]) -> Result<PolicyState, HealthCodecError> {
    decode_versioned(bytes, POLICY_STATE_SCHEMA_VERSION, "PolicyState")
}

pub fn serialize_epoch_settlement(value: &EpochSettlement) -> Result<Vec<u8>, HealthCodecError> {
    encode_versioned(value, EPOCH_SETTLEMENT_SCHEMA_VERSION, "EpochSettlement")
}

pub fn deserialize_epoch_settlement(bytes: &[u8]) -> Result<EpochSettlement, HealthCodecError> {
    decode_versioned(bytes, EPOCH_SETTLEMENT_SCHEMA_VERSION, "EpochSettlement")
}

pub fn serialize_policy_parameters(value: &PolicyParameters) -> Result<Vec<u8>, HealthCodecError> {
    encode_versioned(value, POLICY_PARAMETERS_SCHEMA_VERSION, "PolicyParameters")
}

pub fn deserialize_policy_parameters(bytes: &[u8]) -> Result<PolicyParameters, HealthCodecError> {
    decode_versioned(bytes, POLICY_PARAMETERS_SCHEMA_VERSION, "PolicyParameters")
}

fn encode_versioned<T: Serialize>(
    payload: &T,
    version: u16,
    artifact: &'static str,
) -> Result<Vec<u8>, HealthCodecError> {
    let envelope = VersionedEnvelopeRef { version, payload };
    bincode::serialize(&envelope).map_err(|error| HealthCodecError::SerializationFailure {
        artifact,
        message: error.to_string(),
    })
}

fn decode_versioned<T: DeserializeOwned>(
    bytes: &[u8],
    expected_version: u16,
    artifact: &'static str,
) -> Result<T, HealthCodecError> {
    let envelope: VersionedEnvelope<T> =
        bincode::deserialize(bytes).map_err(|error| HealthCodecError::SerializationFailure {
            artifact,
            message: error.to_string(),
        })?;
    if envelope.version != expected_version {
        return Err(HealthCodecError::UnsupportedSchemaVersion {
            artifact,
            expected: expected_version,
            actual: envelope.version,
        });
    }
    Ok(envelope.payload)
}

#[cfg(test)]
mod tests {
    use super::{
        deserialize_health_input_snapshot, serialize_health_input_snapshot, HealthCodecError,
        VersionedEnvelope,
    };
    use crate::rust::health::types::HealthInputSnapshot;

    #[test]
    fn codec_round_trips_health_input_snapshot() {
        let snapshot = HealthInputSnapshot {
            target_epoch: 5,
            submitted_at_millis: 777,
            metrics_version: 1,
            payload_hash: [4; 32],
            payload_bytes: vec![1, 2, 3, 4],
            signer_set_id: 2,
            quorum_verified: true,
            accepted: true,
        };

        let encoded = serialize_health_input_snapshot(&snapshot).expect("must encode");
        let decoded = deserialize_health_input_snapshot(&encoded).expect("must decode");
        assert_eq!(decoded, snapshot);
    }

    #[test]
    fn codec_rejects_unknown_schema_version() {
        let envelope = VersionedEnvelope {
            version: 99u16,
            payload: HealthInputSnapshot {
                target_epoch: 5,
                submitted_at_millis: 777,
                metrics_version: 1,
                payload_hash: [4; 32],
                payload_bytes: vec![1, 2, 3, 4],
                signer_set_id: 2,
                quorum_verified: true,
                accepted: true,
            },
        };
        let encoded = bincode::serialize(&envelope).expect("must encode");

        let decoded = deserialize_health_input_snapshot(&encoded);
        assert_eq!(
            decoded,
            Err(HealthCodecError::UnsupportedSchemaVersion {
                artifact: "HealthInputSnapshot",
                expected: 1,
                actual: 99,
            })
        );
    }
}
