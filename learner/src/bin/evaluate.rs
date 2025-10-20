use automatafl_learner::*;
use burn::backend::ndarray::NdArrayDevice;
use burn_ndarray::NdArray;
use clap::Parser;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;

type Backend = NdArray;

/// Evaluate two Automatafl models against each other
#[derive(Parser, Debug)]
#[command(name = "evaluate")]
#[command(about = "Evaluate two models by playing games against each other")]
struct Args {
    /// Path to first model checkpoint directory
    #[arg(long)]
    model1: PathBuf,

    /// Iteration number for model 1 (or 'best' for best model)
    #[arg(long)]
    iter1: String,

    /// Path to second model checkpoint directory (defaults to same as model1)
    #[arg(long)]
    model2: Option<PathBuf>,

    /// Iteration number for model 2 (or 'best' for best model)
    #[arg(long)]
    iter2: String,

    /// Network configuration file
    #[arg(short, long, default_value = "config.toml")]
    config: PathBuf,

    /// Number of games to play
    #[arg(short, long, default_value = "100")]
    num_games: usize,

    /// MCTS simulations per move
    #[arg(long, default_value = "800")]
    mcts_sims: usize,
}

#[derive(Debug, serde::Deserialize)]
struct Config {
    network: NetworkConfig,
}

#[derive(Debug, serde::Deserialize)]
struct NetworkConfig {
    num_blocks: usize,
    num_channels: usize,
    policy_channels: usize,
    value_channels: usize,
}

fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let args = Args::parse();

    // Load network configuration
    info!("Loading configuration from {:?}", args.config);
    let config_str = std::fs::read_to_string(&args.config)?;
    let config: Config = toml::from_str(&config_str)?;

    let net_config = NetConfig {
        num_blocks: config.network.num_blocks,
        num_channels: config.network.num_channels,
        policy_channels: config.network.policy_channels,
        value_channels: config.network.value_channels,
    };

    // Initialize device
    let device = NdArrayDevice::Cpu;
    info!("Using device: {:?}", device);

    // Load model 1
    info!("Loading model 1 from {:?}", args.model1);
    let checkpoint_mgr1 = CheckpointManager::new(args.model1.clone(), 0)?;
    let model1 = if args.iter1 == "best" {
        info!("Loading best model");
        checkpoint_mgr1.load_best(&net_config, &device)?
    } else {
        let iter = args.iter1.parse::<usize>()?;
        info!("Loading checkpoint iteration {}", iter);
        let (model, _, _) = checkpoint_mgr1.load(iter, &net_config, &device)?;
        model
    };

    // Load model 2
    let model2_path = args.model2.as_ref().unwrap_or(&args.model1);
    info!("Loading model 2 from {:?}", model2_path);
    let checkpoint_mgr2 = CheckpointManager::new(model2_path.clone(), 0)?;
    let model2 = if args.iter2 == "best" {
        info!("Loading best model");
        checkpoint_mgr2.load_best(&net_config, &device)?
    } else {
        let iter = args.iter2.parse::<usize>()?;
        info!("Loading checkpoint iteration {}", iter);
        let (model, _, _) = checkpoint_mgr2.load(iter, &net_config, &device)?;
        model
    };

    // Wrap in Arc
    let model1 = Arc::new(model1);
    let model2 = Arc::new(model2);

    // Configure MCTS for evaluation
    let mcts_config = MCTSConfig {
        num_simulations: args.mcts_sims,
        c_puct: 1.0,
        ..Default::default()
    };

    // Run evaluation
    info!("Running evaluation: {} games with {} MCTS simulations",
          args.num_games, args.mcts_sims);
    info!("Model 1 vs Model 2");

    let result = Evaluator::evaluate(
        model1,
        model2,
        device,
        mcts_config,
        args.num_games,
    );

    // Print results
    println!("\n=== Evaluation Results ===");
    println!("Total games: {}", args.num_games);
    println!("Model 1 wins: {} ({:.1}%)",
             result.wins,
             result.wins as f32 / args.num_games as f32 * 100.0);
    println!("Model 2 wins: {} ({:.1}%)",
             result.losses,
             result.losses as f32 / args.num_games as f32 * 100.0);
    println!("Draws: {} ({:.1}%)",
             result.draws,
             result.draws as f32 / args.num_games as f32 * 100.0);
    println!("\nModel 1 win rate: {:.2}%", result.win_rate() * 100.0);

    if result.should_accept(0.55) {
        println!("\n✓ Model 1 wins decisively (>55% win rate)");
    } else if result.win_rate() > 0.5 {
        println!("\n~ Model 1 wins slightly but not decisively");
    } else if result.win_rate() < 0.45 {
        println!("\n✗ Model 2 wins decisively (Model 1 win rate <45%)");
    } else {
        println!("\n~ Models are evenly matched");
    }

    Ok(())
}
