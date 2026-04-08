use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::rust::health::types::PPM_DENOMINATOR;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyParameters {
    pub burn_fraction_min_ppm: u64,
    pub burn_fraction_max_ppm: u64,
    pub reserve_emission_min_ppm: u64,
    pub reserve_emission_max_ppm: u64,
    pub min_regime_dwell_epochs: u64,
    pub upward_transition_hysteresis_ppm: u64,
    pub downward_transition_hysteresis_ppm: u64,
    pub allow_emergency_override: bool,
}

impl PolicyParameters {
    pub fn validate(&self) -> Result<(), PolicyParameterError> {
        if self.burn_fraction_min_ppm > self.burn_fraction_max_ppm {
            return Err(PolicyParameterError::InvalidBurnRange {
                min: self.burn_fraction_min_ppm,
                max: self.burn_fraction_max_ppm,
            });
        }
        if self.reserve_emission_min_ppm > self.reserve_emission_max_ppm {
            return Err(PolicyParameterError::InvalidReserveRange {
                min: self.reserve_emission_min_ppm,
                max: self.reserve_emission_max_ppm,
            });
        }
        if self.burn_fraction_max_ppm > PPM_DENOMINATOR {
            return Err(PolicyParameterError::BurnMaxOutOfBounds {
                max: self.burn_fraction_max_ppm,
            });
        }
        if self.reserve_emission_max_ppm > PPM_DENOMINATOR {
            return Err(PolicyParameterError::ReserveMaxOutOfBounds {
                max: self.reserve_emission_max_ppm,
            });
        }
        if self.upward_transition_hysteresis_ppm > PPM_DENOMINATOR {
            return Err(PolicyParameterError::UpwardHysteresisOutOfBounds {
                value: self.upward_transition_hysteresis_ppm,
            });
        }
        if self.downward_transition_hysteresis_ppm > PPM_DENOMINATOR {
            return Err(PolicyParameterError::DownwardHysteresisOutOfBounds {
                value: self.downward_transition_hysteresis_ppm,
            });
        }
        Ok(())
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PolicyParameterError {
    #[error("burn range invalid: min={min}, max={max}")]
    InvalidBurnRange { min: u64, max: u64 },
    #[error("reserve range invalid: min={min}, max={max}")]
    InvalidReserveRange { min: u64, max: u64 },
    #[error("burn max out of bounds: max={max}")]
    BurnMaxOutOfBounds { max: u64 },
    #[error("reserve max out of bounds: max={max}")]
    ReserveMaxOutOfBounds { max: u64 },
    #[error("upward hysteresis out of bounds: value={value}")]
    UpwardHysteresisOutOfBounds { value: u64 },
    #[error("downward hysteresis out of bounds: value={value}")]
    DownwardHysteresisOutOfBounds { value: u64 },
}

#[cfg(test)]
mod tests {
    use super::{PolicyParameterError, PolicyParameters};

    #[test]
    fn policy_parameters_validate_rejects_invalid_range() {
        let params = PolicyParameters {
            burn_fraction_min_ppm: 10,
            burn_fraction_max_ppm: 5,
            reserve_emission_min_ppm: 0,
            reserve_emission_max_ppm: 1,
            min_regime_dwell_epochs: 1,
            upward_transition_hysteresis_ppm: 0,
            downward_transition_hysteresis_ppm: 0,
            allow_emergency_override: false,
        };

        assert_eq!(
            params.validate(),
            Err(PolicyParameterError::InvalidBurnRange { min: 10, max: 5 })
        );
    }
}
