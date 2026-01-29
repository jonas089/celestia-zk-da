//! Standalone verifier CLI for proof chains.
//!
//! This binary verifies the entire proof chain from Celestia,
//! ensuring all proofs are valid and roots are consistent.

use anyhow::Result;
use celestia_adapter::Namespace;
use clap::{Parser, Subcommand};
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;
use verifier_lib::{ChainVerifier, VerifyConfig};

#[derive(Parser)]
#[command(name = "verifier")]
#[command(about = "Verify proof chains from Celestia DA")]
struct Cli {
    /// Celestia RPC URL
    #[arg(long, default_value = "http://localhost:26658")]
    celestia_rpc: String,

    /// Namespace to verify
    #[arg(long, default_value = "zkapp")]
    namespace: String,

    /// Log level
    #[arg(long, default_value = "info")]
    log_level: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Verify transitions in a height range
    Verify {
        /// Starting height
        #[arg(long)]
        from: u64,
        /// Ending height
        #[arg(long)]
        to: u64,
        /// Skip proof verification (only check root chain)
        #[arg(long)]
        skip_proofs: bool,
        /// Expected first root (hex)
        #[arg(long)]
        expected_root: Option<String>,
    },
    /// Check if Celestia node is ready
    Status,
    /// Get current head height
    Head,
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

    let config = VerifyConfig {
        celestia_rpc: cli.celestia_rpc,
        namespace: Namespace::from_string(&cli.namespace),
        ..Default::default()
    };

    match cli.command {
        Commands::Verify {
            from,
            to,
            skip_proofs,
            expected_root,
        } => {
            verify_range(config, from, to, skip_proofs, expected_root).await?;
        }
        Commands::Status => {
            check_status(config).await?;
        }
        Commands::Head => {
            get_head(config).await?;
        }
    }

    Ok(())
}

async fn verify_range(
    mut config: VerifyConfig,
    from: u64,
    to: u64,
    skip_proofs: bool,
    expected_root: Option<String>,
) -> Result<()> {
    config.skip_proof_verification = skip_proofs;

    if let Some(root_hex) = expected_root {
        let root_bytes = hex::decode(&root_hex)?;
        if root_bytes.len() != 32 {
            anyhow::bail!("expected root must be 32 bytes");
        }
        let mut root = [0u8; 32];
        root.copy_from_slice(&root_bytes);
        config.expected_first_root = Some(root);
    }

    info!("Verifying transitions from height {} to {}", from, to);

    let verifier = ChainVerifier::new(config);

    // Check connection first
    if !verifier.is_ready().await {
        anyhow::bail!("Celestia node is not ready");
    }

    match verifier.verify_range(from, to).await {
        Ok(result) => {
            println!("\n=== Verification Complete ===");
            println!("Total transitions verified: {}", result.total_transitions);
            println!(
                "Sequence range: {} - {}",
                result.first_sequence, result.last_sequence
            );
            println!(
                "Height range: {} - {}",
                result.height_range.0, result.height_range.1
            );
            println!("First root: {}", hex::encode(result.first_root));
            println!("Latest root: {}", hex::encode(result.latest_root));

            if !result.unverified_transitions.is_empty() {
                println!(
                    "\nWarning: {} transitions had no proof (not verified):",
                    result.unverified_transitions.len()
                );
                for seq in &result.unverified_transitions {
                    println!("  - Sequence {}", seq);
                }
            }

            println!("\nStatus: OK");
        }
        Err(e) => {
            println!("\n=== Verification Failed ===");
            println!("Error: {}", e);
            println!("\nStatus: FAILED");
            std::process::exit(1);
        }
    }

    Ok(())
}

async fn check_status(config: VerifyConfig) -> Result<()> {
    let verifier = ChainVerifier::new(config);

    print!("Checking Celestia node... ");
    if verifier.is_ready().await {
        println!("READY");

        let height = verifier.head_height().await?;
        println!("Current head height: {}", height);
    } else {
        println!("NOT READY");
        std::process::exit(1);
    }

    Ok(())
}

async fn get_head(config: VerifyConfig) -> Result<()> {
    let verifier = ChainVerifier::new(config);

    let height = verifier.head_height().await?;
    println!("{}", height);

    Ok(())
}
