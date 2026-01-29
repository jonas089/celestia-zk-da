//! Example Finance Application with ZK-Proven Transfers
//!
//! This example demonstrates how to build a finance application where:
//! 1. All state transitions are proven in ZK
//! 2. Business logic (valid transfers) is verified inside the circuit
//! 3. Proofs are posted to Celestia DA
//! 4. Anyone can verify the full proof chain

use anyhow::Result;
use app_da_node::AppNodeClient;
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use state::StateOp;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;
use transition_format::{OperationType, VerifiableOperation};

/// Account state in the finance app.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Account {
    pub balance: u64,
    pub nonce: u64,
}

impl Account {
    pub fn encode(&self) -> Vec<u8> {
        bincode::serialize(self).expect("encoding should not fail")
    }

    pub fn decode(data: &[u8]) -> Option<Self> {
        bincode::deserialize(data).ok()
    }
}

#[derive(Parser)]
#[command(name = "finance")]
#[command(about = "Example finance application with ZK-proven transfers")]
struct Cli {
    /// API server URL
    #[arg(long, default_value = "http://127.0.0.1:16000")]
    api_url: String,

    /// Log level
    #[arg(long, default_value = "info")]
    log_level: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create an account with initial balance
    CreateAccount {
        /// Account name
        #[arg(long)]
        name: String,
        /// Initial balance
        #[arg(long)]
        balance: u64,
    },
    /// Transfer funds between accounts
    Transfer {
        /// Sender account
        #[arg(long)]
        from: String,
        /// Receiver account
        #[arg(long)]
        to: String,
        /// Amount to transfer
        #[arg(long)]
        amount: u64,
    },
    /// Show account balance
    Balance {
        /// Account name
        name: String,
    },
    /// Show all accounts
    Accounts,
    /// Show current state root and history
    Status,
    /// Run demo with multiple operations
    Demo,
}

fn account_key(name: &str) -> Vec<u8> {
    format!("account:{}", name).into_bytes()
}

async fn get_account(client: &AppNodeClient, name: &str) -> Result<Option<Account>> {
    let key = account_key(name);
    match client.get(&key).await? {
        Some(data) => Ok(Account::decode(&data)),
        None => Ok(None),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env file
    dotenv::dotenv().ok();

    let cli = Cli::parse();

    // Setup logging
    let level = match cli.log_level.to_lowercase().as_str() {
        "trace" => Level::TRACE,
        "debug" => Level::DEBUG,
        "info" => Level::INFO,
        "warn" => Level::WARN,
        "error" => Level::ERROR,
        _ => Level::INFO,
    };

    let subscriber = FmtSubscriber::builder().with_max_level(level).finish();
    tracing::subscriber::set_global_default(subscriber)?;

    // Create HTTP client
    let client = AppNodeClient::new(cli.api_url);

    // Test connection
    if !client.health().await.unwrap_or(false) {
        anyhow::bail!("Failed to connect to API server. Make sure the server is running.");
    }

    match cli.command {
        Commands::CreateAccount { name, balance } => {
            create_account(client, &name, balance).await?;
        }
        Commands::Transfer { from, to, amount } => {
            transfer(client, &from, &to, amount).await?;
        }
        Commands::Balance { name } => {
            show_balance(client, &name).await?;
        }
        Commands::Accounts => {
            show_accounts(client).await?;
        }
        Commands::Status => {
            show_status(client).await?;
        }
        Commands::Demo => {
            run_demo(client).await?;
        }
    }

    Ok(())
}

async fn create_account(client: AppNodeClient, name: &str, balance: u64) -> Result<()> {
    // Check if account already exists
    if get_account(&client, name).await?.is_some() {
        anyhow::bail!("Account '{}' already exists", name);
    }

    let account = Account { balance, nonce: 0 };
    let key = account_key(name);

    // Create operation with verification data
    let ops = vec![StateOp::Insert {
        key: key.clone(),
        value: account.encode(),
    }];

    // Create verifiable operation for circuit
    let verifiable_ops = vec![VerifiableOperation {
        op_type: OperationType::CreateAccount {
            initial_balance: balance,
        },
        key: key.clone(),
        old_value: None,
        new_value: Some(account.encode()),
        witness_index: 0,
    }];

    let public_inputs = format!("create_account:{}:{}", name, balance).into_bytes();

    info!("Creating account '{}' with balance {}", name, balance);

    let result = client
        .apply_transition(ops, public_inputs, vec![], verifiable_ops)
        .await?;

    println!("Account created:");
    println!("  Name: {}", name);
    println!("  Balance: {}", balance);
    println!("  Sequence: {}", result.sequence);
    println!("  Root: {}", hex::encode(result.new_root));

    Ok(())
}

async fn transfer(client: AppNodeClient, from: &str, to: &str, amount: u64) -> Result<()> {
    // Get sender account
    let from_account = get_account(&client, from)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Sender account '{}' not found", from))?;

    // Check balance
    if from_account.balance < amount {
        anyhow::bail!(
            "Insufficient balance: {} has {}, needs {}",
            from,
            from_account.balance,
            amount
        );
    }

    // Get or create receiver account
    let to_account = get_account(&client, to).await?.unwrap_or_default();

    // Compute new balances
    let from_new = Account {
        balance: from_account.balance - amount,
        nonce: from_account.nonce + 1,
    };
    let to_new = Account {
        balance: to_account.balance + amount,
        nonce: to_account.nonce,
    };

    let from_key = account_key(from);
    let to_key = account_key(to);

    let ops = vec![
        StateOp::Insert {
            key: from_key.clone(),
            value: from_new.encode(),
        },
        StateOp::Insert {
            key: to_key.clone(),
            value: to_new.encode(),
        },
    ];

    // Create verifiable operation for the transfer
    let verifiable_ops = vec![VerifiableOperation {
        op_type: OperationType::Transfer {
            from: from_key.clone(),
            to: to_key.clone(),
            amount,
        },
        key: from_key.clone(),
        old_value: Some(from_account.encode()),
        new_value: Some(from_new.encode()),
        witness_index: 0,
    }];

    let public_inputs = format!("transfer:{}:{}:{}", from, to, amount).into_bytes();

    info!("Transferring {} from '{}' to '{}'", amount, from, to);

    let result = client
        .apply_transition(ops, public_inputs, vec![], verifiable_ops)
        .await?;

    println!("Transfer complete:");
    println!(
        "  From: {} ({} -> {})",
        from, from_account.balance, from_new.balance
    );
    println!(
        "  To: {} ({} -> {})",
        to, to_account.balance, to_new.balance
    );
    println!("  Amount: {}", amount);
    println!("  Sequence: {}", result.sequence);
    println!("  Root: {}", hex::encode(result.new_root));

    Ok(())
}

async fn show_balance(client: AppNodeClient, name: &str) -> Result<()> {
    let key = account_key(name);
    let (value, proof) = client.get_with_proof(&key).await?;
    let root = client.root().await?;

    match value {
        Some(data) => {
            if let Some(account) = Account::decode(&data) {
                println!("Account: {}", name);
                println!("  Balance: {}", account.balance);
                println!("  Nonce: {}", account.nonce);
                println!("  Proof valid: {}", proof.verify(&root));
                println!("  Root: {}", hex::encode(root));
            } else {
                println!("Error: Could not decode account data");
            }
        }
        None => {
            println!("Account '{}' not found", name);
        }
    }

    Ok(())
}

async fn show_accounts(client: AppNodeClient) -> Result<()> {
    println!("=== Accounts ===");
    let root_info = client.get_latest_root().await?;
    println!("Root: {}", hex::encode(root_info.root));
    println!();

    // Note: This is a simplified version. A full implementation would need
    // an API endpoint to list all accounts or we'd need to track account names separately.
    println!("To see individual accounts, use the 'balance' command.");
    println!("Example: cargo run --bin finance balance alice");

    Ok(())
}

async fn show_status(client: AppNodeClient) -> Result<()> {
    println!("=== Finance App Status ===");
    let root_info = client.get_latest_root().await?;
    println!("Root: {}", hex::encode(root_info.root));
    println!("Transition index: {}", root_info.transition_index);
    println!();

    println!("=== Root History ===");
    for (seq, root, height) in client.root_history().await? {
        print!("  {}: {}", seq, hex::encode(root));
        if let Some(h) = height {
            print!(" (celestia: {})", h);
        }
        println!();
    }

    Ok(())
}

async fn run_demo(client: AppNodeClient) -> Result<()> {
    println!("=== Finance App Demo ===\n");

    // Create accounts
    println!("--- Creating Accounts ---");

    let accounts = [("alice", 1000u64), ("bob", 500), ("charlie", 250)];

    for (name, balance) in &accounts {
        let account = Account {
            balance: *balance,
            nonce: 0,
        };
        let key = account_key(name);

        let ops = vec![StateOp::Insert {
            key: key.clone(),
            value: account.encode(),
        }];

        let verifiable_ops = vec![VerifiableOperation {
            op_type: OperationType::CreateAccount {
                initial_balance: *balance,
            },
            key: key.clone(),
            old_value: None,
            new_value: Some(account.encode()),
            witness_index: 0,
        }];

        let public_inputs = format!("create:{}:{}", name, balance).into_bytes();

        let result = client
            .apply_transition(ops, public_inputs, vec![], verifiable_ops)
            .await?;
        println!(
            "Created {}: balance={}, root={}",
            name,
            balance,
            hex::encode(&result.new_root[..8])
        );
    }

    println!();

    // Perform transfers
    println!("--- Transfers ---");

    let transfers = [
        ("alice", "bob", 200u64),
        ("bob", "charlie", 100),
        ("charlie", "alice", 50),
    ];

    for (from, to, amount) in &transfers {
        let from_acc = get_account(&client, from).await?.unwrap();
        let to_acc = get_account(&client, to).await?.unwrap_or_default();

        let from_key = account_key(from);
        let to_key = account_key(to);

        let from_new = Account {
            balance: from_acc.balance - amount,
            nonce: from_acc.nonce + 1,
        };
        let to_new = Account {
            balance: to_acc.balance + amount,
            nonce: to_acc.nonce,
        };

        let ops = vec![
            StateOp::Insert {
                key: from_key.clone(),
                value: from_new.encode(),
            },
            StateOp::Insert {
                key: to_key.clone(),
                value: to_new.encode(),
            },
        ];

        let verifiable_ops = vec![VerifiableOperation {
            op_type: OperationType::Transfer {
                from: from_key.clone(),
                to: to_key.clone(),
                amount: *amount,
            },
            key: from_key,
            old_value: Some(from_acc.encode()),
            new_value: Some(from_new.encode()),
            witness_index: 0,
        }];

        let public_inputs = format!("transfer:{}:{}:{}", from, to, amount).into_bytes();

        let result = client
            .apply_transition(ops, public_inputs, vec![], verifiable_ops)
            .await?;
        println!(
            "Transfer {} -> {} ({}): root={}",
            from,
            to,
            amount,
            hex::encode(&result.new_root[..8])
        );
    }

    println!();

    // Show final balances
    println!("--- Final Balances ---");

    let root = client.root().await?;
    for (name, _) in &accounts {
        let (value, proof) = client.get_with_proof(&account_key(name)).await?;
        if let Some(data) = value {
            if let Some(acc) = Account::decode(&data) {
                println!(
                    "{}: balance={}, nonce={}, proof_valid={}",
                    name,
                    acc.balance,
                    acc.nonce,
                    proof.verify(&root)
                );
            }
        }
    }

    println!();

    // Show history
    println!("--- Transition History ---");
    for (seq, root, height) in client.root_history().await? {
        print!("Seq {}: {}", seq, hex::encode(&root[..8]));
        if let Some(h) = height {
            print!(" (celestia: {})", h);
        }
        println!();
    }

    println!("\n=== Demo Complete ===");
    println!("Final root: {}", hex::encode(client.root().await?));

    Ok(())
}
