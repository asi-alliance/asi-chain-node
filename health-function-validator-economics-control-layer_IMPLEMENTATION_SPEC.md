# Codex-Ready Implementation Spec
## ASI:Chain Health Function & Validator Economics Control Layer
### Target: f1r3node integration

Version: 0.1
Date: 2026-04-08

---

## 1. Objective

Implement a deterministic, auditable health-policy control layer for ASI:Chain on top of the current f1r3node architecture.

The implementation must:

- maintain a canonical on-chain health policy state
- support bounded externally submitted health inputs
- apply health-driven burn/emission/reserve effects at epoch boundaries
- preserve validator determinism and block validity without requiring off-chain live queries
- keep the audited consensus-critical surface as small as possible

This spec is designed to be executable by Codex as a multi-epic implementation plan.

---

## 2. Product Requirements Summary

### Required outcomes
1. The chain accepts canonical health inputs for future epochs.
2. The chain computes or applies a canonical health regime from those inputs.
3. Epoch settlement applies:
   - burn effects
   - emission enable/disable
   - reserve emission adjustments
   - validator reward pool shaping
4. Validators only rely on finalized chain state plus accepted canonical inputs.
5. External metrics never directly mutate balances or consensus behavior.
6. Audit scope is explicitly separated from modular infrastructure.

### Non-goals
- direct proposer-right changes based on reputation
- per-block health recomputation
- dependence on Prometheus/indexers for block validity
- unrestricted automatic parameter mutation
- final reputation-weighted validator economics in v1

---

## 3. Architecture Overview

## 3.1 Core principle

Use a hybrid architecture:

- **On-chain canonical policy**
- **Externally computed bounded inputs**
- **Epoch-based economic settlement**

## 3.2 Main components

### Consensus-critical / audited
1. `HealthInputOracle`
2. `HealthPolicyController`
3. `EpochEconomicSettlement`
4. `PolicyEvents`
5. permissioning / signer quorum / emergency override logic

### Modular / non-core
1. external metrics pipeline
2. simulation engine
3. dashboard / explorer health views
4. backtesting and analytics
5. governance UX tooling

---

## 4. Repo-Oriented Component Placement

> Note: final file/module locations may vary depending on current branch layout. Codex should inspect the active rust/dev tree and align with existing patterns for system contracts, PoS config, and runtime services.

### Proposed logical ownership

#### A. System contract / protocol state layer
Use for:
- canonical health input snapshots
- health policy state
- epoch settlement state
- policy output storage

#### B. Epoch transition / settlement integration
Use for:
- invoking settlement logic at epoch boundaries
- reading finalized fee totals
- applying mint/burn/reward/reserve routing

#### C. API/event exposure layer
Use for:
- surfacing current health regime
- surfacing pending next-epoch policy
- surfacing settlement results
- supporting explorer/indexer consumption

#### D. Off-chain metrics/oracle service
Use for:
- computing external terms such as cross-shard correlation
- packaging signed input bundles
- simulating effects before submission

---

## 5. Epics

## Epic 1 — Canonical Health Input Ingestion

### Goal
Create a narrow interface through which externally computed health inputs can be submitted and accepted for a future epoch.

### Deliverables
- `HealthInputSnapshot` state model
- submission endpoint/entrypoint
- schema validation
- signer/quorum validation
- freshness validation
- bounded metric validation
- activation scheduling for future epoch
- accepted/rejected event emission

### User stories
- As the protocol, I need to accept externally computed health inputs without making live off-chain queries part of consensus.
- As a validator, I need to verify accepted health inputs from chain state only.
- As governance, I need bounded control over which inputs can affect policy.

### Tasks
1. Define `HealthInputSnapshot` schema.
2. Define metric payload versioning.
3. Define signer/quorum rules.
4. Implement submission validation.
5. Implement state persistence for pending inputs.
6. Emit accepted/rejected events.
7. Add query endpoints for latest pending/active snapshots.

### Acceptance criteria
- invalid payloads are rejected deterministically
- stale payloads are rejected deterministically
- payloads cannot activate in the current epoch
- validators reconstruct accepted state from chain data alone
- duplicate submissions are handled deterministically

---

## Epic 2 — Health Policy State Machine

### Goal
Create the canonical health-policy controller that combines on-chain terms and accepted external terms into a regime and policy outputs.

### Deliverables
- `PolicyState`
- health regime enum
- score calculation or regime mapping logic
- output parameter derivation
- anti-flapping controls
- fallback behavior

### Proposed regimes
- `HEALTHY`
- `CAUTIOUS`
- `STRESSED`
- `CRITICAL`
- `HIBERNATION`

### Suggested policy outputs
- `burn_fraction`
- `emission_mode`
- `reserve_emission_rate`
- `reserve_circuit_breaker`
- optional future `validator_reward_multiplier_cap`

### Tasks
1. Define policy input set.
2. Classify each term:
   - on-chain native
   - external submitted
   - governance-set
3. Implement regime transition logic.
4. Implement hysteresis / dwell-time behavior.
5. Implement fallback behavior for missing external inputs.
6. Emit policy activation events.

### Acceptance criteria
- same prior state + same inputs => same output for all validators
- policy cannot oscillate excessively under small input variation
- fallback behavior is deterministic
- policy activation occurs only at allowed epoch boundary

---

## Epic 3 — Epoch Economic Settlement

### Goal
At epoch boundary, apply the active policy to fee totals, emissions, reserve flows, and validator reward pool allocation.

### Deliverables
- `EpochSettlement`
- fee aggregation input reader
- burn execution
- emission application
- reserve routing
- validator reward pool routing
- settlement events

### Tasks
1. Identify canonical finalized fee counters available at epoch close.
2. Implement settlement entrypoint.
3. Compute burned amount from `burn_fraction`.
4. Apply emission or hibernation rule.
5. Route validator reward pool.
6. Route treasury / insurance / staking allocations if in scope.
7. Update reserve balances.
8. Persist settlement record.
9. Emit settlement event.

### Acceptance criteria
- settlement results are deterministic
- supply changes match exact accounting equations
- hibernation produces zero scheduled emission
- reserve updates reflect active `reserve_emission_rate`
- settlement cannot run twice for the same epoch

---

## Epic 4 — Validator Reward Pool Integration

### Goal
Integrate health-driven global economic shaping into validator economics without coupling health directly to consensus rights.

### v1 Rule
Health changes:
- total emissions
- burn level
- reserve payout behavior
- total validator reward pool composition

Health does **not** change:
- proposer eligibility
- voting power rules
- consensus safety assumptions

### Tasks
1. Identify current reward distribution flow in PoS/system contracts.
2. Add settlement-fed reward pool inputs.
3. Preserve existing deterministic reward distribution semantics.
4. Add optional future extension point for bounded reputation multiplier.

### Acceptance criteria
- validator rewards remain deterministic
- health affects reward pool size, not consensus permissions
- no change to proposer/right selection logic in v1

---

## Epic 5 — Events, Queries, and Observability

### Goal
Make health input, policy transitions, and settlement fully observable by indexers, explorer surfaces, and operators.

### Deliverables
- events for submission accepted/rejected
- events for policy activation
- events for epoch settlement
- query endpoints for current and next policy state
- query endpoints for last settlement

### Tasks
1. Define event schema.
2. Add state query endpoints.
3. Ensure explorer/indexer compatibility.
4. Add regression tests for event completeness.

### Acceptance criteria
- all economically relevant transitions are surfaced
- explorer/indexer can reconstruct health timeline
- event payloads are versioned and stable

---

## Epic 6 — Shadow Mode and Simulation

### Goal
Before economic enforcement, support shadow-mode validation of policy behavior using live or replayed data.

### Deliverables
- off-chain simulator
- input bundle builder
- scenario runner
- side-by-side comparison between advisory and enforceable results

### Tasks
1. Implement policy simulator using same formulas.
2. Support replay from historical/fake epoch datasets.
3. Add expected policy regime outputs for benchmark scenarios.
4. Produce shadow-mode reports.

### Acceptance criteria
- simulator matches on-chain controller behavior for shared logic
- policy outcomes can be previewed before enforcement
- edge-case scenarios are testable offline

---

## 6. Data Models

## 6.1 HealthInputSnapshot

```rust
struct HealthInputSnapshot {
    target_epoch: u64,
    submitted_at_millis: u64,
    metrics_version: u32,
    payload_hash: [u8; 32],
    payload_bytes: Vec<u8>,
    signer_set_id: u32,
    quorum_verified: bool,
    accepted: bool,
}
```

### Notes
- final serialization format should match the node/system-contract standards already used in the repo
- `payload_bytes` may later become strongly typed if schema stabilizes early

---

## 6.2 PolicyState

```rust
enum HealthRegime {
    Healthy,
    Cautious,
    Stressed,
    Critical,
    Hibernation,
}

enum EmissionMode {
    Normal,
    Hibernating,
}

struct PolicyState {
    epoch_id: u64,
    activation_epoch: u64,
    source_snapshot_hash: [u8; 32],
    health_score_ppm: u64,
    health_regime: HealthRegime,
    burn_fraction_ppm: u64,
    reserve_emission_rate_ppm: u64,
    emission_mode: EmissionMode,
    reserve_circuit_breaker: bool,
    status_flags: u64,
}
```

---

## 6.3 EpochSettlement

```rust
struct EpochSettlement {
    epoch_id: u64,
    total_fees: u128,
    burned_amount: u128,
    validator_reward_amount: u128,
    staking_reward_amount: u128,
    treasury_amount: u128,
    insurance_amount: u128,
    reserve_emission_amount: u128,
    minted_emission_amount: u128,
    hibernation_applied: bool,
    settlement_hash: [u8; 32],
}
```

---

## 6.4 Governance Parameters

```rust
struct PolicyParameters {
    burn_fraction_min_ppm: u64,
    burn_fraction_max_ppm: u64,
    reserve_emission_min_ppm: u64,
    reserve_emission_max_ppm: u64,
    min_regime_dwell_epochs: u64,
    upward_transition_hysteresis_ppm: u64,
    downward_transition_hysteresis_ppm: u64,
    allow_emergency_override: bool,
}
```

---

## 7. Input Classification Matrix

| Input / Term | Source Type | Consensus-Critical? | Notes |
|---|---|---:|---|
| Finalized epoch fees | On-chain | Yes | Used in settlement |
| Reserve balances | On-chain | Yes | Used in reserve update |
| Scheduled emission amount | On-chain / governance | Yes | Must be deterministic |
| Burn fraction | On-chain derived | Yes | Output of policy |
| Reserve emission rate | On-chain derived | Yes | Output of policy |
| Emission mode | On-chain derived | Yes | Output of policy |
| Cross-shard correlation `Ψ_t` | External submitted | Yes after acceptance | Must enter through bounded interface |
| Wash-trade adjusted fee quality | External submitted | Yes after acceptance | Optional initial phase |
| Thresholds / weights | Governance-set | Yes | Versioned parameter set |

---

## 8. State Transitions

## 8.1 Input Snapshot Lifecycle

```text
DRAFTED_EXTERNALLY
  -> SUBMITTED
  -> VALIDATED
  -> ACCEPTED_PENDING
  -> ACTIVATED
  -> CONSUMED
```

### Rejections

```text
SUBMITTED -> REJECTED_STALE
SUBMITTED -> REJECTED_INVALID_SCHEMA
SUBMITTED -> REJECTED_OUT_OF_BOUNDS
SUBMITTED -> REJECTED_INSUFFICIENT_QUORUM
SUBMITTED -> REJECTED_INVALID_TARGET_EPOCH
```

---

## 8.2 Policy Lifecycle

```text
NO_POLICY
  -> POLICY_PENDING
  -> POLICY_ACTIVE
  -> POLICY_SETTLED
  -> POLICY_ARCHIVED
```

---

## 8.3 Regime Lifecycle

```text
HEALTHY -> CAUTIOUS -> STRESSED -> CRITICAL -> HIBERNATION
```

Recovery path:

```text
HIBERNATION -> CRITICAL -> STRESSED -> CAUTIOUS -> HEALTHY
```

### Anti-flapping rules
- minimum dwell time before downgrade/upgrade
- score smoothing if numerical score is retained
- capped per-epoch change in burn and reserve emission rate
- delayed activation of externally sourced stress signals

---

## 9. Formula Execution Model

## 9.1 Recommended execution model

### Step A — Pre-epoch
An external metrics service prepares a signed input bundle for epoch `N+1`.

### Step B — Input acceptance
The chain validates and stores the bundle as pending.

### Step C — Epoch boundary
At `N -> N+1`, the controller:
- reads active on-chain terms
- reads accepted input bundle for `N+1`
- computes or maps the active policy outputs
- activates `PolicyState(N+1)`

### Step D — Settlement
At epoch close, settlement:
- reads finalized epoch fees
- applies burn
- applies mint/hibernation
- routes validator/staking/treasury/insurance shares
- updates reserves
- emits settlement event

---

## 10. Interfaces

## 10.1 On-chain entrypoints / commands

### `submit_health_input(snapshot)`
Behavior:
- validate schema
- validate signatures/quorum
- validate target epoch
- validate metric bounds
- persist pending snapshot
- emit accepted/rejected event

### `activate_policy_for_epoch(epoch_id)`
Behavior:
- read previous policy
- read active/pending accepted input
- apply formula / regime mapping
- write active policy
- emit policy activation event

### `settle_epoch_economics(epoch_id)`
Behavior:
- assert previous settlement absent
- read active policy
- read finalized fee totals
- compute settlement outputs
- update supply/reserves/reward pools
- emit settlement event

### `query_current_policy()`
Returns current active policy.

### `query_pending_policy(epoch_id)`
Returns pending snapshot/policy preview if available.

### `query_last_settlement()`
Returns latest finalized settlement record.

---

## 10.2 External service interfaces

### Metrics pipeline output
```json
{
  "target_epoch": 123,
  "metrics_version": 1,
  "submitted_at_millis": 1775600000000,
  "metrics": {
    "psi_ppm": 320000,
    "fee_quality_ppm": 910000
  },
  "payload_hash": "0x...",
  "signatures": [
    { "signer": "oracle-1", "sig": "..." },
    { "signer": "oracle-2", "sig": "..." }
  ]
}
```

### Simulation engine input
- historical or synthetic epoch sequence
- metric bundle stream
- baseline parameter set
- expected regime sequence

---

## 11. Audit Boundaries

## 11.1 Must be audited
1. `HealthInputOracle`
2. `HealthPolicyController`
3. `EpochEconomicSettlement`
4. supply accounting and burn execution
5. validator reward pool routing
6. signer/quorum / permissioning / emergency overrides
7. serialization / persistence format used in consensus-critical state

## 11.2 Can remain outside main audit initially
1. metrics computation service
2. dashboards
3. explorer UI
4. backtesting engine
5. analytics notebooks
6. advisory simulations

### Important rule
External systems may be wrong or unavailable, but the chain must still fail safely and deterministically.

---

## 12. Failure Modes and Required Safe Behavior

## Failure Mode A — Missing external bundle
### Required behavior
- use last safe accepted policy or fallback policy
- do not halt chain
- emit stale-input warning event

## Failure Mode B — Outlier metric spike
### Required behavior
- reject if beyond hard bounds
- otherwise cap effect through policy parameter ranges

## Failure Mode C — Oscillating inputs
### Required behavior
- regime dwell time
- hysteresis thresholds
- capped parameter deltas

## Failure Mode D — Settlement replay / duplication
### Required behavior
- idempotency guard by epoch id
- settlement record existence check

## Failure Mode E — Oracle signer compromise
### Required behavior
- multisig/quorum validation
- emergency signer set rotation
- delayed activation for sensitive changes

---

## 13. Testing Strategy

## 13.1 Unit tests

### Health input validation
- accepts valid future-epoch bundle
- rejects stale bundle
- rejects invalid version/schema
- rejects no quorum
- rejects out-of-range metric
- rejects same-epoch activation

### Policy state machine
- same inputs produce same outputs
- each regime boundary is tested
- hysteresis prevents immediate flip-flop
- fallback path tested when input absent

### Settlement accounting
- correct burn application
- correct hibernation behavior
- correct reward routing
- correct reserve update
- correct supply delta

---

## 13.2 Property tests
- deterministic convergence across randomized valid inputs
- no negative balances
- no overflow/underflow
- policy outputs remain within configured min/max
- settlement is idempotent by epoch

---

## 13.3 Integration tests
- submit bundle -> activate policy -> settle epoch
- missing bundle fallback path
- sequential epoch transitions with regime changes
- event stream completeness
- validator/node sync reproduces active state correctly

---

## 13.4 Simulation scenarios

### Scenario 1 — Healthy baseline
Expected:
- baseline burn
- normal emission
- normal reserve emissions

### Scenario 2 — Moderate deterioration
Expected:
- elevated burn
- modest reserve throttling
- emission still normal

### Scenario 3 — Critical stress
Expected:
- strong burn
- reserve circuit-break style behavior
- emission reduced or hibernation depending on thresholds

### Scenario 4 — Recovery after hibernation
Expected:
- bounded stepwise regime recovery
- no instant return to baseline
- hysteresis respected

---

## 14. Codex Implementation Order

## Phase 1 — Scaffolding
1. inspect current rust/dev repo layout
2. locate PoS/system-contract/state integration points
3. create feature branch structure
4. create placeholder modules/types:
   - `health_input`
   - `health_policy`
   - `epoch_settlement`
   - `policy_events`

## Phase 2 — Core models and serialization
1. implement data structures
2. implement serialization/deserialization
3. add version markers
4. add storage bindings / state accessors

## Phase 3 — Health input oracle
1. implement bundle validation
2. implement signer/quorum logic
3. implement pending storage
4. implement query endpoints
5. add unit tests

## Phase 4 — Policy controller
1. implement regime calculation
2. implement output parameter derivation
3. implement hysteresis and dwell logic
4. implement fallback path
5. add tests

## Phase 5 — Epoch settlement
1. wire finalized fee reading
2. compute settlement accounting
3. integrate mint/burn updates
4. integrate reward routing
5. integrate reserve update
6. add tests

## Phase 6 — Events and APIs
1. add accepted/rejected events
2. add policy activation events
3. add settlement events
4. add query endpoints

## Phase 7 — Simulation and shadow mode
1. implement mirror simulator
2. build scenario fixtures
3. compare simulator vs on-chain logic
4. generate readiness report

## Phase 8 — Audit hardening
1. review integer safety
2. review replay/idempotency protections
3. review permissioning
4. review fallback behavior
5. produce audit checklist

---

## 15. File / Module Deliverables

Codex should create or modify modules matching repo conventions, but at minimum deliver equivalents of:

```text
health/
  mod.rs
  types.rs
  input_oracle.rs
  policy_controller.rs
  settlement.rs
  events.rs
  params.rs
  tests/
    input_oracle_tests.rs
    policy_controller_tests.rs
    settlement_tests.rs
    integration_tests.rs
```

If the repo uses contract-specific directories or Rholang/system-contract boundaries, mirror the existing architecture rather than forcing this exact tree.

---

## 16. Open Questions for Human Review

1. Which exact formula terms from the DeAI health paper must be in v1?
2. Is `health_score` stored numerically, or do we only store regime outputs?
3. What is the exact fallback policy if no valid bundle is accepted?
4. Which signer/quorum model is acceptable for TestNet1 vs TestNet2?
5. What is the minimum validator reward floor during hibernation?
6. Which reward-routing buckets are in scope for the first implementation?
7. Is reserve circuit breaker partial throttling or full suspension?

Codex should flag these as blocking decisions where needed, but continue building the deterministic infrastructure around them.

---

## 17. Definition of Done

The feature is done when:

1. valid health input bundles can be accepted for future epochs
2. a canonical policy state activates deterministically at epoch boundaries
3. epoch settlement applies burn/emission/reserve/reward effects deterministically
4. validators do not need off-chain systems to validate chain state
5. events and queries expose all economically relevant transitions
6. test coverage exists for validation, policy transitions, settlement, fallback, and replay protection
7. a narrow audit boundary is documented and implementation-ready

---

## 18. Recommended First Merge Strategy

### Merge 1
Scaffolding + data models + serialization + query-only state

### Merge 2
HealthInputOracle + tests

### Merge 3
HealthPolicyController + tests

### Merge 4
EpochEconomicSettlement + tests

### Merge 5
Events/API integration + shadow mode tooling

### Merge 6
Audit-hardening changes

This sequencing minimizes risk and allows early review before supply-affecting logic lands.

---

## 19. Final Guidance to Codex

When implementing:
- prefer deterministic integer math over floating point
- gate all external inputs through explicit validation
- keep settlement idempotent by epoch
- keep health logic outside per-block validation paths
- treat auditability as a primary design goal
- reuse existing f1r3node config/state/system-contract conventions rather than inventing parallel patterns
- isolate future extensions such as validator reputation weighting behind feature boundaries and leave them disabled in v1
