# Celestia 3.0 — Data Availability for ZKVM Proofs (SP1)  
## Implementation Plan (Single-File, Publishable)

## 0) Summary
We are building a ZK application stack where developers write **business logic in Rust**, and the system proves **state transitions** using **SP1**. Each application maintains its own Merkle-tree state and posts **proofs + new state roots** to **Celestia DA** under an **app-specific namespace**. Anyone can independently verify the full proof chain by querying Celestia. The application also runs an **app-specific DA node** (RETH replacement) that stores the underlying state data and serves **Merkle inclusion proofs** for requested values.

This is not an execution engine. It’s a data + proof pipeline:
- Celestia DA: proofs + roots (verification source of truth)
- App-DA node: actual state data + Merkle proofs (serving layer)

No Groth16 required; use native SP1 proofs.

---

## 1) Goals and Non-Goals

### Goals
- A bank can build arbitrary finance logic by writing only Rust business logic.
- The ZK circuit code is **generic** and supports any state transition of the form:
  - `(prev_root, public_inputs, private_inputs) -> new_root`
- Proofs and state roots are posted to a dedicated Celestia namespace per app.
- Anyone can verify:
  1) each proof is valid, and
  2) the chain of roots is consistent (prev_root matches previous new_root).
- App-DA node stores state data and provides a Merkle proof endpoint so clients can verify values against published roots.
- A sequencer (or coordinator) signs the new root after it is locally verified.

### Non-Goals (for this iteration)
- Groth16 or recursive proof aggregation.
- On-chain settlement or bridging.
- Distributed sequencer consensus (single sequencer is fine initially).
- Fully decentralized state storage (App-DA is initially run by the app operator).

---

## 2) Target Users
- Banks / industrial players building privacy-preserving finance products.
- Their developers should not touch SP1 host/guest complexity.
- SDK should abstract:
  - Merkle tree state handling
  - witness construction
  - proof generation
  - Celestia posting + retrieval
  - proof-chain verification
  - Merkle inclusion proof serving

---

## 3) System Architecture

### 3.1 Components
1) **SDK (Rust)**
   - Developer-facing crate(s) to implement business logic.
   - Generates an SP1-compatible “transition program” artifact.
   - Provides state APIs, Merkle tree utilities, serialization formats.

2) **SP1 Prover/Verifier Harness**
   - Host-side orchestration for proving transitions.
   - Uses `./example-sp1` as reference.
   - Emits:
     - proof bytes
     - transition output (new_root + metadata)

3) **Celestia DA Adapter**
   - Posts blobs to Celestia under a namespace.
   - Queries blobs by height/range/namespace.
   - Provides deterministic encoding (so independent verifiers parse the same bytes).

4) **App-Specific DA Node (RETH replacement)**
   - Stores full application state (key/value database).
   - Maintains Merkle tree and root.
   - Produces SP1 proofs for state transitions.
   - Submits proof blobs to Celestia.
   - Exposes HTTP/gRPC endpoints:
     - query values
     - query Merkle proofs
     - query current root
     - query historical roots (optional)
   - Verifies proofs from Celestia to sync state from scratch (light-client style).

5) **Verifier CLI / Library**
   - Fetches Celestia data for a namespace.
   - Verifies the proof chain end-to-end.
   - Optionally verifies Merkle proofs from App-DA node responses.

### 3.2 Data Flow
1) Developer writes business logic using SDK traits.
2) App-DA node applies a batch of operations / transactions:
   - loads current state root
   - computes updated state (new key/values)
   - updates Merkle tree to get new_root
3) App-DA node constructs SP1 inputs:
   - prev_root (public)
   - public_inputs (public)
   - private_inputs (private)
   - optional: state transition “witness commitments” (see below)
4) SP1 produces proof and outputs new_root.
5) App-DA node validates and signs new_root.
6) App-DA node posts a blob to Celestia namespace:
   - header + proof + public outputs + new_root + metadata
7) Verifier fetches blobs, verifies proofs, checks root continuity.
8) User queries App-DA node for a value + Merkle proof and verifies against a root obtained from Celestia proofs.

---

## 4) State and Proof Model

### 4.1 State Commitment
- State is committed by a Merkle root.
- App-DA node stores full state data in a local DB.
- Tree choice should be swappable, but start with a simple binary Merkle or SMT.

### 4.2 Transition Function
All apps must reduce to:

- Public:
  - `prev_root: [u8; 32]`
  - `public_inputs: Vec<u8>` (typed encoding in SDK)
- Private:
  - `private_inputs: Vec<u8>` (typed encoding in SDK)
- Output:
  - `new_root: [u8; 32]`
  - optional: `public_outputs` (events/receipts digest, etc.)

### 4.3 What the SP1 Program Must Prove
Minimum requirement:
- The transition program is executed correctly given the inputs, producing `new_root`.

Practical requirement:
- The program must be able to recompute `new_root` deterministically from:
  - prev_root + public_inputs + private_inputs
- This implies the program needs enough info to compute the new Merkle root. In practice that means either:
  - (A) include Merkle update witnesses (paths/siblings) as private inputs, or
  - (B) include a deterministic state-update log and verify it against prev_root (still requires witnesses), or
  - (C) include a commitment to a state diff plus proofs of correctness (still needs witnesses).
Start with (A): **private inputs include Merkle update witnesses** sufficient to update the root.

---

## 5) Celestia Data Format (Blob Schema)

### 5.1 Blob Content Requirements
Each posted blob must be self-contained for independent verification:
- version
- app_id (or namespace-derived ID)
- sequence number / transition index
- prev_root
- new_root
- public_inputs (or hash)
- public_outputs (optional)
- proof bytes (SP1 proof)
- program identifier (hash) to bind to specific ZK program
- optional: timestamp / sequencer signature

### 5.2 Encoding
Use a deterministic encoding (do not rely on JSON for canonicalization). Recommended:
- bincode / postcard / protobuf with fixed field ordering and versioning
- include a schema version at the start

Define:
- `TransitionBlobV1` struct with explicit fields
- `hash_transition(blob) -> [u8; 32]` for indexing / signing

---

## 6) Sequencer and Root Signing
- The sequencer (or the App-DA node acting as sequencer) signs:
  - `(transition_index, prev_root, new_root, program_hash, celestia_height, blob_hash)`
- Signature is optional for initial trust model, but should be implemented early because banks will want it.
- Verification pipeline must not depend on the signature for correctness (signature is authorization, proof is correctness).

---

## 7) App-DA Node API

### 7.1 HTTP Endpoints (Minimum)
- `GET /root/latest`
  - returns latest known root + transition index + celestia reference
- `GET /value?key=<...>&at=<root_or_index>`
  - returns value + merkle_proof + root_reference
- `GET /proof/merkle?key=<...>&at=<root_or_index>`
  - returns Merkle inclusion proof only
- `GET /sync/status`
  - returns last verified celestia height, last transition index, latest root
- `GET /health`

### 7.2 Response Format
Use deterministic encoding for proofs (base64 or hex for bytes).
Include:
- `root`
- `value`
- `proof` (Merkle proof nodes + leaf hash scheme)
- `key_hash` / `key_encoding` description
- `hash_fn` and `tree_spec` identifiers

Clients must be able to verify without guesswork.

---

## 8) SDK Design (Developer Experience)

### 8.1 Developer Interface
The developer should implement something like:
- `trait BusinessLogic { fn apply(prev_state: &mut State, public: PublicInputs, private: PrivateInputs) -> PublicOutputs }`
The SDK handles:
- state read/write
- Merkleization
- witness extraction for updated keys
- encoding of inputs for SP1
- proof generation interface

### 8.2 SDK Modules
- `state`:
  - key/value abstractions
  - typed keys
  - serialization
- `merkle`:
  - tree implementation
  - update + proof generation
- `zk`:
  - SP1 input builder
  - program hash binding
  - proof generation wrapper
- `celestia`:
  - namespace operations
  - submit + query
  - blob schema encoding/decoding
- `node`:
  - app-da server framework
  - storage adapters

---

## 9) Implementation Phases (Concrete Tasks)

## Phase 1 — Local Celestia + Minimal DA Adapter
### Goals
- Start local Celestia node.
- Post and retrieve blobs under a namespace.
- Confirm deterministic encoding and parsing.

### Tasks
1) Start Celestia:
- `make start` (provided)
2) Implement `celestia_adapter`:
- `submit_blob(namespace, bytes) -> (height, tx_hash)`
- `get_blobs(namespace, height_range) -> Vec<bytes>`
3) Implement `TransitionBlobV1` encoding/decoding.
4) Write a small integration test:
- submit a dummy TransitionBlob
- fetch it back
- decode and compare bytes/hashes

### Deliverables
- `crates/celestia_adapter`
- `crates/blob_schema`
- `tests/da_roundtrip.rs`

---

## Phase 2 — SP1 Transition Program (Reference → Generic Skeleton)
### Goals
- Use `./example-sp1` as baseline.
- Build a minimal SP1 program that:
  - accepts `(prev_root, public_inputs, private_inputs)`
  - outputs `new_root`
- Initially, compute `new_root` in a trivial way (e.g., hash(prev_root || inputs)) to validate plumbing end-to-end.

### Tasks
1) Clone structure from `./example-sp1`.
2) Define SP1 guest input format (bincode/postcard).
3) Implement guest logic:
- parse inputs
- compute `new_root = H(prev_root || public_inputs || private_inputs)`
- write `new_root` as public output
4) Host harness:
- compile guest
- produce proof
- verify proof
- extract output

### Deliverables
- `crates/zk_guest_transition`
- `crates/zk_host_harness`
- `tests/sp1_proof_smoke.rs`

---

## Phase 3 — Real State: Merkle Tree + Witness-Based Updates
### Goals
- Replace trivial root update with actual Merkle root transition.
- Ensure private inputs carry sufficient witnesses for updated keys.

### Tasks
1) Implement Merkle tree module:
- insert/update
- compute root
- generate inclusion proofs
- generate update witnesses (path + siblings)
2) Define transition format:
- list of operations (public or private)
- required witnesses included in private_inputs
3) Update SP1 guest to:
- verify prev_root matches computed root from provided witnesses + leaf values (for touched keys)
- apply operations to recompute new_root deterministically
4) Host side:
- build witnesses from App-DA state
- feed to SP1

### Deliverables
- `crates/merkle`
- `crates/state`
- `crates/transition_format`
- `tests/merkle_transition_proof.rs`

---

## Phase 4 — App-DA Node (State Server + Proof Producer)
### Goals
- Implement the App-DA node service:
  - stores state
  - applies transitions
  - generates proofs
  - posts to Celestia
  - serves value queries with Merkle proofs

### Tasks
1) Storage layer:
- DB for key/values (sled/rocksdb/sqlite acceptable)
- store latest root, transition index
2) Transition pipeline:
- accept “operation batch” (local for now, API later)
- apply to state
- generate witnesses
- produce SP1 proof
- create TransitionBlobV1
- submit to Celestia
3) HTTP server:
- implement endpoints:
  - `/root/latest`
  - `/value`
  - `/proof/merkle`
  - `/sync/status`
4) Add verification mode:
- node can run in “sync from Celestia” mode:
  - query namespace blobs
  - verify proofs
  - update local root tracking
  - optionally reconstruct state if data available (initially root tracking is enough)

### Deliverables
- `crates/app_da_node`
- `crates/app_da_api`
- `bin/appd` CLI
- `tests/node_e2e.rs`

---

## Phase 5 — Public Verifier CLI (Trustless Checking)
### Goals
- Provide a standalone verifier that:
  - queries Celestia namespace
  - verifies every proof
  - checks root continuity
  - outputs the latest verified root and transition index

### Tasks
1) CLI:
- `verify --namespace <ns> --from <height> --to <height>`
2) Verification:
- decode TransitionBlobV1
- verify SP1 proof against program hash
- check `blob.prev_root == last_new_root`
3) Output summary:
- total transitions verified
- first root
- latest root
- celestia heights covered

### Deliverables
- `bin/verifier`
- `crates/verifier_lib`
- `tests/verifier_chain.rs`

---

## Phase 6 — SDK Polish for “Business Logic Only”
### Goals
- Make the developer experience realistic:
  - no manual witness work
  - typed inputs
  - predictable state APIs
- Provide an example finance app (balances, transfers, compliance check stub).

### Tasks
1) `sdk` crate:
- `State` abstraction
- typed key utilities
- `BusinessLogic` trait
- automatic witness extraction for touched keys
2) Example app:
- accounts + transfers
- private inputs: sender auth/secret
- public inputs: receiver, amount, nonce
- proofs posted to namespace
3) Documentation:
- README describing workflow:
  - run node
  - submit a transfer batch
  - verify with CLI
  - query balances + verify Merkle proof

### Deliverables
- `crates/sdk`
- `examples/finance_app`
- `docs/README.md`

---

## 10) Repo Structure (Recommended)
- `crates/blob_schema`
- `crates/celestia_adapter`
- `crates/merkle`
- `crates/state`
- `crates/transition_format`
- `crates/zk_guest_transition`
- `crates/zk_host_harness`
- `crates/app_da_node`
- `crates/app_da_api`
- `crates/verifier_lib`
- `crates/sdk`
- `bin/appd`
- `bin/verifier`
- `examples/finance_app`
- `tests/*`

---

## 11) Concrete “First Milestone” Definition (What Done Looks Like)
A complete local demo where:

1) `make start` brings up local Celestia.
2) Run `appd` node for an example app namespace.
3) Apply 3 transitions (e.g. create accounts, transfer).
4) For each transition:
- SP1 proof generated and verified locally
- TransitionBlobV1 posted to Celestia namespace
5) Run `verifier` CLI:
- fetches blobs
- verifies all SP1 proofs
- confirms root chain continuity
- prints latest root
6) Query `GET /value?key=account:alice&at=latest`
- returns balance + Merkle proof
7) Client verifies Merkle proof against latest root obtained from `verifier` output.

---

## 12) Notes and Constraints
- Use native SP1 proofs only (no Groth16).
- Deterministic encoding is mandatory (blob schema must be canonical).
- Do not rely on App-DA node honesty:
  - correctness is proven by SP1 proofs posted to Celestia.
- App-DA node is primarily a state serving layer + proof producer; Celestia is the source of truth for verifiability.

---

## 13) Instructions for Claude (Execution Expectations)
- Treat `./example-sp1` as the canonical reference for SP1 guest/host wiring.
- Assume `make start` starts a usable local Celestia node for the entire development session.
- Implement Phase 1 + Phase 2 first to validate the full “prove -> post -> fetch -> verify” loop before building the Merkle logic.
- Keep everything modular: schema + DA adapter + prover harness must be reusable across apps.

