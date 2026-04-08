# Health Control Layer Audit Checklist

## Scope
- [ ] `health/input_oracle.rs`
- [ ] `health/policy_controller.rs`
- [ ] `health/settlement.rs`
- [ ] `health/storage.rs`
- [ ] `health/codec.rs`
- [ ] `health/control_layer.rs`

## Input Oracle
- [ ] Target-epoch validation enforces future-only activation windows.
- [ ] Metric payload schema/version checks are deterministic.
- [ ] Payload hash binding is verified against payload bytes.
- [ ] Bounds checks cover all accepted external metrics.
- [ ] Signer allowlist and quorum checks are deterministic.
- [ ] Duplicate pending submissions are rejected idempotently.
- [ ] Rejected submissions emit stable rejection reasons.

## Policy Controller
- [ ] Same prior state + same inputs => identical `PolicyState`.
- [ ] Regime threshold ordering is validated on config load.
- [ ] Dwell/hysteresis transitions prevent flip-flop behavior.
- [ ] Burn/reserve outputs are bounded by governance min/max.
- [ ] Per-epoch output delta caps are enforced.
- [ ] Fallback policy path is deterministic when snapshot is missing/invalid.
- [ ] Fallback status flags are emitted and queryable.

## Settlement
- [ ] Settlement replay guard blocks duplicate epoch settlement.
- [ ] Policy epoch mismatch is rejected.
- [ ] All arithmetic uses checked operations (no overflow/underflow).
- [ ] Hibernation mode forces zero minted emission.
- [ ] Routing shares are validated and bounded by PPM denominator.
- [ ] Accounting identity is checked before persistence:
- [ ] `burn + validator + staking + treasury + insurance + reserve == fees + minted`
- [ ] Settlement hash is deterministic and stable across nodes.

## Storage and Serialization
- [ ] All consensus-critical structs use versioned codec envelopes.
- [ ] Unsupported schema versions fail closed.
- [ ] Store namespaces are isolated (`health-*`) and explicitly mapped.
- [ ] Query paths return canonical latest/pending records deterministically.

## Control Layer and Observability
- [ ] Activation does not consume pending input on failed activation.
- [ ] Successful activation consumes pending input exactly once.
- [ ] Event stream includes accepted and rejected input outcomes.
- [ ] Query APIs cover current policy, pending policy, and last settlement.
- [ ] Integration tests cover submit -> activate -> settle.

## Remaining Decisions (Human Required)
- [ ] Final v1 health score formula from DeAI paper.
- [ ] Final signer model / key management and rotation workflow.
- [ ] Final routing splits (validator/staking/treasury/insurance).
- [ ] Emergency override authorization and delay semantics.
