# Celestia ZK-DA

Build compliant, privacy-preserving financial applications with cryptographic guarantees. Powered by Celestia data availability and SP1 zero-knowledge proofs.

## The Problem

Financial institutions need:

1. **Regulatory Compliance**: Every transaction must be auditable and provably correct
2. **Privacy**: Client data and proprietary business logic must remain confidential
3. **Decentralization**: No single point of failure or trust dependency

Traditional solutions force you to pick two.

## The Solution

This framework combines Celestia's data availability layer with zero-knowledge proofs to deliver all three.

### How It Works

```
┌─────────────────┐     ┌──────────────────┐     ┌─────────────────┐
│  Your Business  │────▶│   ZK Circuit     │────▶│   Celestia DA   │
│     Logic       │     │   (SP1 Prover)   │     │   (Namespace)   │
│   (Rust Code)   │     │                  │     │                 │
└─────────────────┘     └──────────────────┘     └─────────────────┘
        │                       │                        │
        ▼                       ▼                        ▼
   Private Data          Proof of Correct          Public Verification
   Stays Private         State Transition          Anyone Can Audit
```

1. **Write business logic in Rust**: No cryptography expertise required
2. **Proofs generated automatically**: SP1 compiles your logic to ZK circuits
3. **Post to Celestia**: Proofs and state roots published to your app-specific namespace
4. **Anyone can verify**: Cryptographic guarantee that every state transition is valid

## Why Celestia?

### App-Specific Namespaces
Each financial application gets its own namespace. Your data is isolated from other applications while sharing the security of the entire network.

### Sovereign Verification
Your application's rules aren't enforced by the DA layer. They're proven cryptographically and published to Celestia. You maintain complete sovereignty over your business logic while gaining global verifiability.

### Cost Efficiency
Pay only for the data you publish. Celestia's blob-based pricing is cheaper than smart contract execution for proof publication.

### Light Client Verification
Banks, regulators, and auditors can run lightweight verification nodes that cryptographically verify your entire transaction history without processing raw data.

## Privacy Architecture

### What's Public (Posted to Celestia)
- State roots (32-byte commitments)
- Zero-knowledge proofs
- Operation metadata (if desired)
- Sequencer signatures

### What's Private (Never Leaves Your Infrastructure)
- Actual account balances
- Transaction details
- Customer information
- Business logic implementation
- Compliance rule specifics

Regulators can verify correctness without seeing the data. They receive cryptographic proof that:
- All transfers respect balance constraints
- All compliance rules were followed
- State transitions are mathematically valid

## For Financial Institutions

### Write Business Logic, Not Cryptography

```rust
// Your compliance rules, automatically proven in ZK
fn process_transfer(ctx: &mut Context, transfer: Transfer) -> Result<()> {
    let sender = ctx.get_account(&transfer.from)?;
    let receiver = ctx.get_account(&transfer.to)?;

    // Business rule: Check daily limits
    require!(transfer.amount <= sender.daily_limit);

    // Business rule: KYC verification
    require!(sender.kyc_verified && receiver.kyc_verified);

    // Business rule: Sanctions screening
    require!(!is_sanctioned(&transfer.from) && !is_sanctioned(&transfer.to));

    // Execute transfer
    ctx.transfer(&transfer.from, &transfer.to, transfer.amount)?;

    Ok(())
}
```

The SDK handles:
- Merkle tree state management
- Witness extraction for proofs
- ZK circuit compilation
- Celestia blob publication
- Proof verification

Your team focuses on compliance logic. We handle the cryptography.

### Audit-Ready by Design

Every state transition produces:
1. **Cryptographic proof**: Mathematical guarantee of correctness
2. **Celestia commitment**: Timestamped, immutable publication
3. **Merkle proof**: For any individual account query

Auditors and regulators can independently verify:
- All transactions followed your published rules
- No unauthorized state changes occurred
- Complete transaction ordering is preserved
- Historical states are recoverable

## Technical Architecture

### Components

| Component | Purpose |
|-----------|---------|
| `sdk` | Developer-facing Rust crate for business logic |
| `merkle` | Sparse Merkle Tree for state commitment |
| `zk_guest_transition` | SP1 circuit for state transition verification |
| `celestia_adapter` | Blob submission and retrieval |
| `app_da_node` | Full node with HTTP API for state queries |
| `verifier` | Standalone proof chain verification CLI |

### Data Flow

```
1. Business Logic      → State changes identified
2. Witness Extraction  → Merkle proofs for touched keys
3. SP1 Execution       → ZK proof generated
4. Blob Creation       → Proof + root + metadata packaged
5. Celestia Submit     → Published to app namespace
6. Verification        → Anyone can cryptographically verify
```

### API Endpoints

```
GET /health                                      → Node health check
GET /root/latest                                 → Current state root + Celestia reference
GET /value?key=...                               → Value + Merkle proof
GET /proof/merkle?key=...                        → Merkle inclusion proof only
GET /sync/status                                 → Sync status with Celestia
GET /history                                     → Root history with Celestia heights
GET /celestia/transition?height=...              → Fetch transition proof from Celestia
GET /celestia/transitions?from_height=...&to_height=... → Fetch range of proofs
```

## Getting Started

### Prerequisites
- Rust toolchain
- SP1 toolchain (`cargo prove`)
- Docker (for local Celestia)
- Node.js 18+ (for the web frontend)

### Quick Start

```bash
# Start local Celestia network
make start

# Start the API server (this owns the database and serves state)
cargo run --release --bin appd -- \
  --data-dir ./finance-data \
  --namespace finance \
  serve --bind 127.0.0.1:16000

# In another terminal, run the finance demo (sends requests to API server)
cargo run --release --bin finance -- --api-url http://127.0.0.1:16000 demo

# Verify the proof chain
cargo run --release --bin verifier verify --from 1 --to 100
```

### Architecture: Client-Server Model

The system uses a client-server architecture to prevent database locking issues:

- **API Server (`appd`)**: Owns the database exclusively, generates proofs, posts to Celestia
- **Finance App (`finance`)**: HTTP client that sends operations to the API server
- **Verifier**: Reads proofs from Celestia for independent verification

This ensures only one process accesses the database, preventing sled's file lock conflicts.

### Running the API Server

The API server must be running before you can use the finance app:

```bash
# Start API server for the finance app
cargo run --release --bin appd -- \
  --data-dir ./finance-data \
  --namespace finance \
  serve --bind 127.0.0.1:16000
```

### Using the Finance App

With the API server running, you can use the finance CLI:

```bash
# Create accounts
cargo run --bin finance -- --api-url http://127.0.0.1:16000 create-account --name alice --balance 1000
cargo run --bin finance -- --api-url http://127.0.0.1:16000 create-account --name bob --balance 500

# Transfer funds
cargo run --bin finance -- --api-url http://127.0.0.1:16000 transfer --from alice --to bob --amount 100

# Check balance
cargo run --bin finance -- --api-url http://127.0.0.1:16000 balance alice

# View status
cargo run --bin finance -- --api-url http://127.0.0.1:16000 status

# Run full demo
cargo run --bin finance -- --api-url http://127.0.0.1:16000 demo
```

### Querying State via HTTP API

You can also query state directly via the HTTP API:

```bash
# Health check
curl http://localhost:16000/health

# Get current state root
curl http://localhost:16000/root/latest

# Get account balance with Merkle proof
curl "http://localhost:16000/value?key=account:alice"

# Get transition history (shows Celestia heights)
curl http://localhost:16000/history
```

### Running the Web Frontend

A React-based proof explorer is included for visualizing state, accounts, and ZK proofs:

```bash
# Install dependencies (first time only)
cd frontend
npm install

# Start the development server
npm run dev
```

The frontend runs at `http://localhost:5173` and connects to the API server at `http://127.0.0.1:16000`.

**Features:**
- **Dashboard**: Node status, transition count, state root, and history
- **Accounts**: Create accounts, view balances, and inspect Merkle proofs
- **Transfers**: Execute transfers with real-time proof generation feedback
- **Proof Explorer**: Browse all transitions, view SP1 proofs, and Celestia DA details

Make sure the API server is running before starting the frontend.

### Retrieving Proofs from Celestia

Fetch ZK proofs that were posted to Celestia:

```bash
# Get a single transition by Celestia block height
curl "http://localhost:16000/celestia/transition?height=12345"

# Get multiple transitions in a height range
curl "http://localhost:16000/celestia/transitions?from_height=100&to_height=200"
```

Response includes:
- `sequence`: Transition number
- `prev_root`, `new_root`: State roots before and after
- `proof`: The ZK proof (base64 encoded)
- `program_hash`: SP1 circuit hash for verification
- `celestia_height`: Block height where proof is stored

### Build Your Own Application

```rust
use sdk::{Application, Context, Result};

struct MyComplianceApp;

impl Application for MyComplianceApp {
    type PublicInput = TransactionRequest;
    type PrivateInput = ComplianceData;
    type Output = TransactionReceipt;

    fn apply(
        &self,
        ctx: &mut Context,
        request: Self::PublicInput,
        compliance: Self::PrivateInput,
    ) -> Result<Self::Output> {
        // Your business logic here
        // All state changes automatically tracked
        // ZK proof generated on commit
    }
}
```

## Why This Matters

### For Banks
- **Reduce compliance costs**: Automated cryptographic audit trails
- **Protect client privacy**: Prove correctness without exposing data
- **Maintain control**: Your rules, your infrastructure, your sovereignty

### For Regulators
- **Real-time verification**: Cryptographic guarantees, not trust
- **Reduced audit burden**: Proofs speak for themselves
- **No data exposure needed**: Verify without accessing sensitive information

### For the Industry
- **New cooperation models**: Multiple institutions can verify each other's state without sharing data
- **Interoperability foundation**: Standardized proof format for cross-institution verification

## Production Considerations

### Security
- SP1 proofs are based on STARK cryptography
- Celestia provides economic security through staking
- Merkle proofs are cryptographically binding

#### Quantum Resistance: Future-Proof Security

**This project is quantum-resistant. Most blockchain systems are not.**

SP1 uses **STARK** (Scalable Transparent ARgument of Knowledge) cryptography, which relies on hash functions and information-theoretic security. Hash-based cryptography is believed to be secure against quantum computer attacks.

In contrast, **Groth16** and similar SNARK proof systems (used by most blockchain ZK applications) depend on **elliptic curve pairings** and the discrete logarithm problem. These cryptographic primitives are **vulnerable to Shor's algorithm** running on sufficiently powerful quantum computers.

**What this means:**
- **This system**: Proofs remain secure even if quantum computers become practical
- **Groth16-based systems**: Will require migration to quantum-resistant cryptography
- **Most blockchains**: Current signature schemes (ECDSA, EdDSA) are quantum-vulnerable

For financial institutions planning 10-20 year infrastructure lifecycles, quantum resistance is not just a theoretical concern—it's a practical requirement. This architecture is designed to remain secure through the quantum transition.

### Performance
- Proof generation: Parallelizable across machines
- Celestia publication: Sub-second finality
- Verification: Milliseconds per proof

### Compliance
- Designed for financial regulatory requirements
- Audit trail preserved on Celestia indefinitely
- Supports selective disclosure patterns

## Architecture Decisions

### Why SP1?
- Write circuits in Rust, not custom DSLs
- Compilation from standard business logic
- No trusted setup required (STARKs)
- Active development and support

### Why Celestia?
- Purpose-built for rollup/app data availability
- App-specific namespaces for data isolation
- Low DA costs
- Strong light client verification

### Why This Architecture?
- Complete privacy for business logic
- No gas costs for execution complexity
- Full sovereignty over state machine rules
- Regulatory-friendly design