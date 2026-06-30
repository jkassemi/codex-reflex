# Style Guide

This project is designed for **agentic legibility**: a capable but memoryless agent should be able to enter the repository cold, infer intent from structure, make a bounded change, and prove conformance without relying on private context, tribal knowledge, or stale prose documentation.

Documentation is useful only when it clarifies durable intent. The primary sources of truth are, in order:

1. **Specification** — what behavior is required.
2. **Types and public APIs** — how the behavior is made explicit.
3. **Conformance tests** — proof that the behavior is satisfied.
4. **Code structure** — where responsibility lives.
5. **Narrative documentation** — why non-obvious decisions exist.

The code should be easy to read, easy to locate, easy to test, and easy to change without preserving accidental implementation details.

---

## 1. Core Principles

### 1.1 Optimize for agentic legibility

A change should be obvious to an agent that has no memory of prior conversations.

Prefer:

- predictable file locations;
- direct names;
- small modules with clear responsibility;
- explicit domain types;
- tests that map to specification clauses;
- simple control flow;
- local reasoning;
- boring public APIs.

Avoid:

- clever abstractions without clear payoff;
- hidden behavior through macros, blanket impls, or build scripts;
- tests that lock in private implementation details;
- comments that merely restate code;
- behavior that exists only because “that is how it currently works.”

### 1.2 The specification owns behavior

No behavior-bearing code should exist without a corresponding specification reason.

When adding or changing behavior, update the relevant spec first or in the same change. If a behavior is not in the spec, one of the following must be true:

- the behavior is removed;
- the spec is updated to include it;
- the code is marked as purely internal and not externally meaningful.

### 1.3 Tests protect contracts, not internals

Tests should assert externally meaningful behavior. They should not preserve private helper structure, call order, or incidental data layout.

The unit of testing is a **semantic boundary**, not a function.

Good test subjects:

- public crate APIs;
- CLI behavior;
- HTTP/API contracts;
- parser and serializer contracts;
- state-machine transitions;
- storage semantics;
- authorization rules;
- compatibility guarantees;
- migration behavior;
- error contracts visible to callers.

Poor test subjects:

- private helper functions with no independent semantic contract;
- exact internal call order;
- temporary intermediate values;
- cache shape;
- private data structure layout;
- implementation-specific branching.

### 1.4 100% coverage means spec coverage first

This project targets **100% specification coverage**.

Line coverage is useful only as a secondary signal. If uncovered code is found, do not blindly write a test for the uncovered branch. First determine which of these is true:

1. the branch implements a specified behavior and needs a conformance test;
2. the branch reveals a missing specification clause;
3. the branch is dead, overgeneralized, or accidental and should be deleted.

A test that increases line coverage but does not protect specified behavior is not valuable.

### 1.5 Consider operational risks within reason

Every feature that records data, runs automatically, talks to external systems, or sits on a hot path must include a bounded operational-risk pass before it is considered complete.

At minimum, consider:

- storage growth, retention, compaction, and export/purge behavior;
- concurrency, locking, retry, timeout, and partial-failure behavior;
- latency budgets for hooks, CLIs, servers, and user-facing workflows;
- privacy and redaction failure modes, including new secret formats;
- compatibility with versioned schemas, migrations, and real payload variants;
- observability through status, doctor, logs, or inspection commands;
- install/update behavior and what fails if generated binaries or dependencies are absent.

The answer does not need to be heavyweight. Small local tools can use simple limits, bounded retries, documented defaults, and focused tests. What is not acceptable is shipping unbounded data growth, silent data loss, hidden lock contention, or secret exposure because the happy path worked.

---

## 2. Repository Structure

Use predictable locations. A new contributor or agent should be able to guess where a change belongs.

Recommended layout:

```text
.
├── Cargo.toml
├── Cargo.lock
├── README.md
├── docs/
│   ├── STYLE.md
│   ├── SPEC.md
│   ├── ARCHITECTURE.md
│   └── decisions/
├── crates/
│   ├── domain/
│   ├── protocol/
│   ├── storage/
│   ├── storage-<backend>/
│   ├── api/
│   ├── cli/
│   ├── testkit/
│   └── xtask/
├── tests/
│   ├── conformance/
│   ├── integration/
│   ├── compatibility/
│   └── fixtures/
└── tools/
```

Adjust names to the project, but keep the intent clear.

### 2.1 Crate responsibilities

Keep crate boundaries honest.

- `domain`: pure business rules and domain types. No network, database, filesystem, Tokio runtime, or framework dependencies.
- `protocol`: wire types, schemas, serialization, versioning, compatibility rules.
- `storage`: storage traits and durable semantics.
- `storage-<backend>`: concrete storage implementations.
- `api`: HTTP or RPC surface.
- `cli`: command-line interface.
- `testkit`: shared fixtures, builders, fake implementations, and conformance helpers.
- `xtask`: repository automation.

Core behavior should not depend on adapters. Adapters depend inward on core behavior.

### 2.2 Module structure

Within each crate:

```text
src/
├── lib.rs
├── error.rs
├── model.rs
├── service.rs
├── policy.rs
└── <feature>/
    ├── mod.rs
    ├── model.rs
    ├── service.rs
    └── error.rs
```

Use names that describe domain responsibility, not implementation mechanics.

Prefer:

```text
auth/session.rs
billing/invoice.rs
storage/ledger.rs
```

Avoid vague names:

```text
utils.rs
helpers.rs
misc.rs
common.rs
manager.rs
processor.rs
```

A `utils` module is allowed only when every item in it is truly generic and has no better domain home. Most `utils` modules should be split or renamed.

---

## 3. Specifications

Specifications define required behavior. They are not marketing docs and not implementation notes.

Each normative requirement should have a stable identifier:

```text
AUTH-001
AUTH-002
STORAGE-001
API-001
```

A requirement should be concise, testable, and externally meaningful.

Example:

```text
AUTH-017
A suspended user must not be able to create a new session.
```

Avoid vague requirements:

```text
Authentication should be secure.
The cache should be efficient.
The parser should work well.
```

### 3.1 Spec/test traceability

Every conformance test should identify the requirement it protects.

Example:

```rust
// SPEC: AUTH-017
// A suspended user must not be able to create a new session.
#[test]
fn suspended_user_cannot_create_session() {
    // ...
}
```

When a test protects multiple requirements, list each one.

```rust
// SPEC: AUTH-017, AUDIT-004
```

When a requirement is not testable, rewrite the requirement until it is testable.

### 3.2 Behavior changes

A behavior change must include:

- the changed spec clause;
- conformance tests for the changed behavior;
- implementation changes;
- migration or compatibility notes when applicable.

A pull request or agent change that modifies behavior without touching the spec is incomplete unless the behavior is purely internal.

---

## 4. Rust Code Style

### 4.1 Prefer clarity over cleverness

Rust allows powerful abstractions. Use them when they reduce total complexity, not when they merely show expertise.

Prefer straightforward code:

```rust
let user = users.find(user_id)?;
let decision = policy.authorize(&user, Action::CreateSession)?;

if decision.is_denied() {
    return Err(Error::PermissionDenied);
}
```

Avoid compressing meaningful steps into dense chains when intermediate names would clarify intent.

### 4.2 Make invalid states unrepresentable

Use types to encode domain distinctions.

Prefer:

```rust
pub struct UserId(Uuid);
pub struct SessionId(Uuid);

pub enum AccountStatus {
    Active,
    Suspended { reason: SuspensionReason },
    Closed,
}
```

Avoid:

```rust
pub fn create_session(user_id: String, active: bool) -> Result<String, Error>
```

Use newtypes for identifiers, validated strings, raw versus escaped content, money, quantities, durations, and other values that must not be mixed accidentally.

### 4.3 Avoid boolean blindness

Do not use boolean arguments when the meaning is not obvious at the call site.

Avoid:

```rust
create_session(user, true)?;
```

Prefer:

```rust
create_session(user, SessionMode::Persistent)?;
```

### 4.4 Keep public APIs boring

Public APIs should be narrow, explicit, and unsurprising.

Prefer:

```rust
pub fn authorize(
    actor: &Actor,
    action: Action,
    resource: &Resource,
) -> Result<Decision, AuthError>
```

Avoid:

```rust
pub fn check(actor: &str, action: &str, resource: &str, strict: bool) -> Result<bool, Error>
```

A public API should reveal:

- what domain it belongs to;
- what inputs are valid;
- what errors can occur;
- what behavior is promised;
- what remains internal and changeable.

### 4.5 Prefer explicit conversions

Use `From` and `Into` only for conversions that are obvious, lossless, and unsurprising.

Use named constructors for validation or semantic conversion:

```rust
EmailAddress::parse(input)?;
Money::from_minor_units(cents, Currency::Usd)?;
```

Avoid hiding validation, allocation, lossy conversion, or permission changes behind generic conversion traits.

### 4.6 Avoid unnecessary generics

Use generics when they express a real abstraction. Do not make code generic only because it can be generic.

Prefer concrete types until there is a demonstrated need for abstraction.

Good uses of generics:

- storage traits at adapter boundaries;
- parser over input sources;
- testkit conformance suites;
- domain abstractions with multiple real implementations.

Poor uses of generics:

- one implementation exists and no second one is expected;
- trait bounds obscure simple behavior;
- generic code makes error messages or control flow harder to understand.

### 4.7 Keep lifetimes ordinary

Use borrowing to make ownership clear and avoid unnecessary allocation. Do not expose complex lifetimes in public APIs unless zero-copy behavior is a core requirement.

Prefer owned return types in public APIs when they materially reduce caller complexity.

### 4.8 Macros and generated code

Macros are allowed when they remove repetitive, mechanical code. They are discouraged when they hide domain behavior.

A macro must have:

- a clear name;
- a small surface area;
- tests at the expanded behavior boundary;
- no surprising side effects.

Generated code must be placed in an expected location and must include a comment identifying the source and regeneration command.

---

## 5. Error Handling

### 5.1 Libraries use typed errors

Library crates should expose meaningful error types.

Prefer:

```rust
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("user is suspended")]
    Suspended,

    #[error("permission denied")]
    PermissionDenied,

    #[error("session store failed")]
    Store(#[from] StoreError),
}
```

Avoid returning opaque errors from public library APIs unless the crate is explicitly application-level.

### 5.2 Applications add context

Application crates, CLIs, and binaries may use contextual error handling when the caller is a human or process boundary.

Errors should include enough context to diagnose the failure without exposing secrets.

### 5.3 Panics

Panics are acceptable only for:

- programmer errors;
- impossible states protected by prior validation;
- test failures;
- initialization failures where recovery is not meaningful.

Any public API that may panic must document the condition.

Do not use panics for recoverable domain errors.

---

## 6. Async, Concurrency, and State

### 6.1 Keep async at the edges when possible

Domain logic should usually be synchronous and pure. Async belongs at IO boundaries: network, storage, timers, subprocesses, and external services.

Prefer:

```text
domain logic: sync, deterministic
service orchestration: async when needed
adapters: async IO
```

This makes core behavior easier to test and reason about.

### 6.2 Make cancellation and backpressure explicit

Async code should make the following visible:

- what can be cancelled;
- what happens on cancellation;
- where backpressure is applied;
- whether work is retried;
- what timeout applies;
- how errors are surfaced.

Avoid detached tasks unless their lifetime and shutdown behavior are explicit.

### 6.3 Shared state

Prefer ownership and message passing over shared mutable state.

When shared state is necessary:

- keep lock scopes small;
- do not hold locks across `.await` unless specifically justified;
- name the invariant protected by the lock;
- avoid global mutable state.

---

## 7. Unsafe Code

Unsafe code is a proof obligation.

Unsafe is allowed only when:

- there is a concrete requirement that safe Rust cannot satisfy adequately;
- the unsafe block is minimal and local;
- a safe abstraction is exposed when possible;
- invariants are documented with a `SAFETY:` comment;
- tests, fuzzing, Miri, or other checks are added when appropriate.

Example:

```rust
// SAFETY: `ptr` is created from a valid mutable reference above, remains aligned,
// and no other references are used while this write occurs.
unsafe {
    ptr.write(value);
}
```

Do not use unsafe to avoid learning the safe API, silence the borrow checker prematurely, or micro-optimize without measurement.

---

## 8. Testing Style

### 8.1 Test taxonomy

Use tests according to their purpose:

```text
tests/conformance/     Behavior required by the specification.
tests/integration/     Multiple components working together.
tests/compatibility/   Backward/forward compatibility and migrations.
tests/fixtures/        Shared test data.
crates/*/src tests     Local tests for semantic units only.
```

### 8.2 Conformance tests

Conformance tests are the most important tests in this project.

They must:

- cite spec IDs;
- test externally meaningful behavior;
- avoid private implementation details;
- use clear scenario names;
- fail with useful messages;
- be deterministic.

Prefer test names like:

```rust
suspended_user_cannot_create_session
expired_token_is_rejected
ledger_transfer_preserves_total_balance
v1_payload_decodes_under_v2_schema
```

Avoid names like:

```rust
test_auth_1
works
handles_case
calls_validator
```

### 8.3 Property tests

Property tests are encouraged when the property is a specification-level invariant.

Good property tests:

- encode algebraic or domain laws;
- check parser/serializer round trips;
- verify authorization monotonicity;
- test state-machine invariants;
- check migration idempotence;
- ensure no panics on arbitrary input.

Bad property tests:

- encode the current algorithm;
- assert private intermediate state;
- duplicate implementation logic;
- lock in incidental ordering unless ordering is specified.

### 8.4 Snapshot tests

Snapshot tests are allowed when reviewing structured output is useful.

Snapshots must not be used to freeze accidental behavior. A snapshot update must be reviewed as a behavior change unless it is clearly formatting-only and non-normative.

### 8.5 Mocks and fakes

Prefer fakes with semantic behavior over mocks that assert call order.

Mocks may be used at external boundaries, but avoid tests that pass only because a private method was called in a particular sequence.

### 8.6 Test data builders

Use builders for test setup when they improve readability.

Builder defaults should describe the ordinary valid case.

Example:

```rust
let user = UserFixture::active().build();
let suspended = UserFixture::suspended().build();
```

Avoid large anonymous fixtures that hide what matters to the test.

### 8.7 Coverage policy

Coverage gates should be interpreted as follows:

- uncovered specified behavior is a defect;
- uncovered defensive code requires a spec, justification, or deletion;
- uncovered generated code may be excluded if generation is separately validated;
- unreachable code should be removed or made explicit.

Do not add tests solely to execute lines. Add tests to prove behavior.

---

## 9. Comments and Documentation

### 9.1 Comments explain why

Comments should explain non-obvious intent, invariants, tradeoffs, safety, compatibility, or security reasoning.

Good comment:

```rust
// Check suspension before password verification so a suspended account
// does not reveal whether the submitted password is valid.
```

Bad comment:

```rust
// Check if the user is suspended.
```

### 9.2 Public docs

Public items should have documentation when they are part of a stable API or when misuse is plausible.

Docs should include:

- what the item does;
- when to use it;
- errors;
- panics;
- safety requirements for unsafe APIs;
- examples when helpful.

Do not write long prose that duplicates the implementation.

### 9.3 Architecture docs

Architecture docs should be short and durable.

They should answer:

- what are the major components;
- what owns which responsibility;
- which dependencies point inward or outward;
- which behavior is stable;
- which behavior is intentionally internal;
- what commands prove correctness.

When architecture docs disagree with code, fix one immediately.

### 9.4 Decision records

Use decision records for choices that future agents might otherwise reverse by accident.

A decision record should include:

- context;
- decision;
- alternatives considered;
- consequences;
- date.

Keep decision records brief.

---

## 10. Dependencies

### 10.1 Dependencies are part of the design

Every dependency adds API surface, compile time, update burden, and supply-chain risk.

Before adding a dependency, consider:

- Is it necessary?
- Is it actively maintained?
- Is the license acceptable?
- Does it pull in heavy transitive dependencies?
- Is it needed in core logic or only at an adapter boundary?
- Can the dependency be feature-gated?

### 10.2 Feature flags

Feature flags are public API for libraries.

Feature flags should be:

- additive;
- documented;
- tested;
- named after capabilities, not dependencies when possible.

Prefer:

```toml
features = {
  "sqlite" = ["dep:sqlx"],
  "json" = ["dep:serde_json"]
}
```

Avoid features whose combinations are untested or contradictory.

### 10.3 Lockfile

Applications should commit `Cargo.lock`.

Libraries may commit `Cargo.lock` when doing so improves reproducibility for CI, examples, or workspace development.

---

## 11. Formatting, Linting, and CI

Formatting is not a discussion. Use the standard formatter.

Expected local checks:

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo doc --no-deps --all-features
```

Projects with feature-heavy crates should also test feature combinations.

Recommended additional checks where applicable:

```sh
cargo test --no-default-features
cargo test --all-features
cargo audit
cargo deny check
cargo semver-checks
cargo nextest run
cargo llvm-cov --all-features --workspace
```

The exact commands for this repository should live in one predictable place, preferably `justfile`, `Makefile`, or `xtask`.

Agents should not have to infer the validation command.

---

## 12. Change Workflow

Every meaningful change should follow this order:

1. Identify the relevant spec clause.
2. Update or add the spec clause if behavior changes.
3. Add or update conformance tests.
4. Implement the smallest clear change.
5. Run formatting, linting, tests, and coverage.
6. Remove dead or overgeneralized code.
7. Update decision records or architecture docs only if durable intent changed.

### 12.1 Change classification

Classify changes explicitly:

```text
Change type:
- internal only
- public API compatible
- public API breaking
- behavior compatible
- behavior breaking
- migration required
- documentation only
```

Agents should be conservative with public and behavioral compatibility.

### 12.2 Pull request checklist

Before merging, verify:

- [ ] The changed behavior maps to spec IDs.
- [ ] Tests assert behavior, not internals.
- [ ] Coverage gaps are explained or removed.
- [ ] Public API changes are intentional.
- [ ] Error behavior is clear.
- [ ] New dependencies are justified.
- [ ] Unsafe code has `SAFETY:` comments and tests where appropriate.
- [ ] Generated code can be regenerated by a documented command.
- [ ] Formatting, linting, tests, and coverage pass.

---

## 13. Agent Instructions

When modifying this repository, act as a memoryless maintainer.

Do not assume intent that is not present in the spec, types, tests, or nearby code.

Before changing behavior:

1. find the relevant specification clause;
2. identify the semantic boundary under test;
3. preserve public compatibility unless the requested change explicitly breaks it;
4. prefer the smallest simple implementation;
5. avoid locking in private implementation details through tests.

When uncertain, prefer making intent explicit in one of these places:

- a type;
- a spec clause;
- a conformance test;
- a module boundary;
- a short invariant comment.

Do not add broad narrative documentation to compensate for unclear code. First make the code, names, types, or tests clearer.

---

## 14. Definition of Excellence

Excellent Rust in this project is:

> Low-surprise code with high-fidelity behavioral contracts.

That means:

- behavior has a specification home;
- modules live where agents expect them;
- types encode domain meaning;
- public APIs are narrow and boring;
- tests prove specified behavior;
- internals remain free to change;
- unsafe code is rare and justified;
- dependencies are intentional;
- validation is automated and obvious;
- documentation explains durable intent, not incidental implementation.

The goal is not to write the cleverest Rust. The goal is to create a codebase that a capable agent can safely understand, modify, and verify.

--

## 15. Local constraints are not product requirements

The implementation environment (offline sandbox, missing toolchains, unavailable libraries, missing credentials, CI restrictions, temporary
build failures, etc.) is not the product environment. 

Do not simplify or redesign the architecture merely because the current execution environment is limited. Make reasonable attempts to
satisfy dependencies.
