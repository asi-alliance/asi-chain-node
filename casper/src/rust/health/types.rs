use serde::{Deserialize, Serialize};

pub const HASH_BYTES_LEN: usize = 32;
pub const PPM_DENOMINATOR: u64 = 1_000_000;
pub const STATUS_FLAG_FALLBACK_INPUT: u64 = 1 << 0;

pub type Hash32 = [u8; HASH_BYTES_LEN];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthRegime {
    Healthy,
    Cautious,
    Stressed,
    Critical,
    Hibernation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EmissionMode {
    Normal,
    Hibernating,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthInputSnapshot {
    pub target_epoch: u64,
    pub submitted_at_millis: u64,
    pub metrics_version: u32,
    pub payload_hash: Hash32,
    pub payload_bytes: Vec<u8>,
    pub signer_set_id: u32,
    pub quorum_verified: bool,
    pub accepted: bool,
}

impl HealthInputSnapshot {
    pub fn is_for_future_epoch(&self, current_epoch: u64) -> bool {
        self.target_epoch > current_epoch
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyState {
    pub epoch_id: u64,
    pub activation_epoch: u64,
    pub source_snapshot_hash: Hash32,
    pub health_score_ppm: u64,
    pub health_regime: HealthRegime,
    pub burn_fraction_ppm: u64,
    pub reserve_emission_rate_ppm: u64,
    pub emission_mode: EmissionMode,
    pub reserve_circuit_breaker: bool,
    pub status_flags: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EpochSettlement {
    pub epoch_id: u64,
    pub total_fees: u128,
    pub burned_amount: u128,
    pub validator_reward_amount: u128,
    pub staking_reward_amount: u128,
    pub treasury_amount: u128,
    pub insurance_amount: u128,
    pub reserve_emission_amount: u128,
    pub minted_emission_amount: u128,
    pub hibernation_applied: bool,
    pub settlement_hash: Hash32,
}
