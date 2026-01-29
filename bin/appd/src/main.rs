//! Application DA node CLI.
//!
//! This binary runs the app-specific DA node that:
//! - Stores application state
//! - Generates ZK proofs for transitions
//! - Posts proofs to Celestia
//! - Serves HTTP API for queries

use anyhow::Result;
use app_da_node::{api::create_router, AppNode, AppNodeConfig};
use celestia_adapter::Namespace;
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use state::StateOp;
use std::net::SocketAddr;
use std::path::PathBuf;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

#[derive(Parser)]
#[command(name = "appd")]
#[command(about = "Application DA node for ZK state management")]
struct Cli {
    /// Data directory for state storage
    #[arg(long, default_value = "./data")]
    data_dir: PathBuf,

    /// Application ID
    #[arg(long, default_value = "zkapp")]
    app_id: String,

    /// Celestia namespace
    #[arg(long, default_value = "zkapp")]
    namespace: String,

    /// Celestia RPC URL
    #[arg(long, default_value = "http://localhost:26658")]
    celestia_rpc: String,

    /// Disable Celestia posting
    #[arg(long)]
    no_celestia: bool,

    /// Disable proof generation (execute only)
    #[arg(long)]
    no_proving: bool,

    /// Log level
    #[arg(long, default_value = "info")]
    log_level: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the HTTP server
    Serve {
        /// Address to bind
        #[arg(long, default_value = "127.0.0.1:16000")]
        bind: SocketAddr,
    },
    /// Apply a transition from JSON
    Apply {
        /// JSON file with operations
        #[arg(long)]
        ops_file: Option<PathBuf>,
        /// Operations as JSON string
        #[arg(long)]
        ops: Option<String>,
    },
    /// Show current state
    Status,
    /// Get a value
    Get {
        /// Key to get
        key: String,
    },
    /// Set a value (creates a transition)
    Set {
        /// Key to set
        key: String,
        /// Value to set
        value: String,
    },
    /// Demo: run example finance operations
    Demo,
}

#[derive(Serialize, Deserialize)]
struct OperationsFile {
    operations: Vec<Operation>,
    public_inputs: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Operation {
    #[serde(rename = "type")]
    op_type: String,
    key: String,
    value: Option<String>,
}

impl From<Operation> for StateOp {
    fn from(op: Operation) -> Self {
        match op.op_type.as_str() {
            "delete" => StateOp::Delete {
                key: op.key.into_bytes(),
            },
            _ => StateOp::Insert {
                key: op.key.into_bytes(),
                value: op.value.unwrap_or_default().into_bytes(),
            },
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
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

    // Create config
    let config = AppNodeConfig {
        data_dir: cli.data_dir,
        app_id: cli.app_id.into_bytes(),
        namespace: Namespace::from_string(&cli.namespace),
        celestia_rpc: cli.celestia_rpc,
        celestia_enabled: !cli.no_celestia,
        proving_enabled: !cli.no_proving,
    };

    match cli.command {
        Commands::Serve { bind } => {
            run_server(config, bind).await?;
        }
        Commands::Apply { ops_file, ops } => {
            apply_transition(config, ops_file, ops).await?;
        }
        Commands::Status => {
            show_status(config).await?;
        }
        Commands::Get { key } => {
            get_value(config, &key).await?;
        }
        Commands::Set { key, value } => {
            set_value(config, &key, &value).await?;
        }
        Commands::Demo => {
            run_demo(config).await?;
        }
    }

    Ok(())
}

async fn run_server(config: AppNodeConfig, bind: SocketAddr) -> Result<()> {
    info!("Starting app node with config: {:?}", config);

    let node = AppNode::new(config).await?;
    let state = node.state();

    info!("Current root: {}", hex::encode(node.root().await));
    info!("Starting HTTP server on {}", bind);

    let router = create_router(state);
    let listener = tokio::net::TcpListener::bind(bind).await?;

    axum::serve(listener, router).await?;

    Ok(())
}

async fn apply_transition(
    config: AppNodeConfig,
    ops_file: Option<PathBuf>,
    ops_json: Option<String>,
) -> Result<()> {
    let node = AppNode::new(config).await?;

    let ops_data: OperationsFile = if let Some(file) = ops_file {
        let content = std::fs::read_to_string(file)?;
        serde_json::from_str(&content)?
    } else if let Some(json) = ops_json {
        serde_json::from_str(&json)?
    } else {
        anyhow::bail!("Must provide either --ops-file or --ops");
    };

    let ops: Vec<StateOp> = ops_data.operations.into_iter().map(Into::into).collect();
    let public_inputs = ops_data
        .public_inputs
        .map(|s| s.into_bytes())
        .unwrap_or_default();

    info!("Applying {} operations...", ops.len());

    let result = node
        .apply_transition(ops, public_inputs, vec![], vec![])
        .await?;

    println!("Transition applied:");
    println!("  Sequence: {}", result.sequence);
    println!("  Prev root: {}", hex::encode(result.prev_root));
    println!("  New root: {}", hex::encode(result.new_root));
    if let Some(height) = result.celestia_height {
        println!("  Celestia height: {}", height);
    }

    Ok(())
}

async fn show_status(config: AppNodeConfig) -> Result<()> {
    let node = AppNode::new(config).await?;

    println!("App Node Status:");
    println!("  Root: {}", hex::encode(node.root().await));
    println!("  Transition index: {}", node.transition_index().await);

    let history = node.root_history().await;
    println!("  Root history ({} entries):", history.len());
    for (seq, root, height) in history.iter().take(10) {
        print!("    {}: {}", seq, hex::encode(root));
        if let Some(h) = height {
            print!(" (celestia: {})", h);
        }
        println!();
    }

    Ok(())
}

async fn get_value(config: AppNodeConfig, key: &str) -> Result<()> {
    let node = AppNode::new(config).await?;

    let (value, proof) = node.get_with_proof(key.as_bytes()).await?;

    println!("Key: {}", key);
    match value {
        Some(v) => {
            println!("Value: {}", String::from_utf8_lossy(&v));
            println!("Value (hex): {}", hex::encode(&v));
        }
        None => {
            println!("Value: <not found>");
        }
    }
    println!("Root: {}", hex::encode(node.root().await));
    println!("Proof valid: {}", proof.verify(&node.root().await));

    Ok(())
}

async fn set_value(config: AppNodeConfig, key: &str, value: &str) -> Result<()> {
    let node = AppNode::new(config).await?;

    let ops = vec![StateOp::Insert {
        key: key.as_bytes().to_vec(),
        value: value.as_bytes().to_vec(),
    }];

    let result = node.apply_transition(ops, vec![], vec![], vec![]).await?;

    println!("Value set:");
    println!("  Key: {}", key);
    println!("  Value: {}", value);
    println!("  New root: {}", hex::encode(result.new_root));

    Ok(())
}

async fn run_demo(config: AppNodeConfig) -> Result<()> {
    info!("Running demo with finance operations...");

    let node = AppNode::new(config).await?;

    // Create accounts
    println!("\n=== Creating accounts ===");
    let ops = vec![
        StateOp::insert("balance:alice", bincode::serialize(&1000u64)?),
        StateOp::insert("balance:bob", bincode::serialize(&500u64)?),
        StateOp::insert("balance:charlie", bincode::serialize(&250u64)?),
    ];

    let result = node
        .apply_transition(ops, b"create_accounts".to_vec(), vec![], vec![])
        .await?;

    println!("Created accounts at sequence {}", result.sequence);
    println!("Root: {}", hex::encode(result.new_root));

    // Transfer 1: Alice -> Bob
    println!("\n=== Transfer: Alice -> Bob (100) ===");

    // Read current balances
    let alice_bal: u64 = node
        .get(b"balance:alice")
        .await?
        .map(|v| bincode::deserialize(&v).unwrap_or(0))
        .unwrap_or(0);
    let bob_bal: u64 = node
        .get(b"balance:bob")
        .await?
        .map(|v| bincode::deserialize(&v).unwrap_or(0))
        .unwrap_or(0);

    let ops = vec![
        StateOp::insert("balance:alice", bincode::serialize(&(alice_bal - 100))?),
        StateOp::insert("balance:bob", bincode::serialize(&(bob_bal + 100))?),
    ];

    let result = node
        .apply_transition(ops, b"transfer:alice:bob:100".to_vec(), vec![], vec![])
        .await?;

    println!("Transfer complete at sequence {}", result.sequence);
    println!("Root: {}", hex::encode(result.new_root));

    // Transfer 2: Bob -> Charlie
    println!("\n=== Transfer: Bob -> Charlie (50) ===");

    let bob_bal: u64 = node
        .get(b"balance:bob")
        .await?
        .map(|v| bincode::deserialize(&v).unwrap_or(0))
        .unwrap_or(0);
    let charlie_bal: u64 = node
        .get(b"balance:charlie")
        .await?
        .map(|v| bincode::deserialize(&v).unwrap_or(0))
        .unwrap_or(0);

    let ops = vec![
        StateOp::insert("balance:bob", bincode::serialize(&(bob_bal - 50))?),
        StateOp::insert("balance:charlie", bincode::serialize(&(charlie_bal + 50))?),
    ];

    let result = node
        .apply_transition(ops, b"transfer:bob:charlie:50".to_vec(), vec![], vec![])
        .await?;

    println!("Transfer complete at sequence {}", result.sequence);
    println!("Root: {}", hex::encode(result.new_root));

    // Show final balances
    println!("\n=== Final Balances ===");
    for account in ["alice", "bob", "charlie"] {
        let key = format!("balance:{}", account);
        let (value, proof) = node.get_with_proof(key.as_bytes()).await?;
        let balance: u64 = value
            .map(|v| bincode::deserialize(&v).unwrap_or(0))
            .unwrap_or(0);
        println!(
            "{}: {} (proof valid: {})",
            account,
            balance,
            proof.verify(&node.root().await)
        );
    }

    // Show history
    println!("\n=== Transition History ===");
    for (seq, root, height) in node.root_history().await {
        print!("Sequence {}: {}", seq, hex::encode(root));
        if let Some(h) = height {
            print!(" (celestia: {})", h);
        }
        println!();
    }

    println!("\n=== Demo Complete ===");
    println!("Final root: {}", hex::encode(node.root().await));

    Ok(())
}
