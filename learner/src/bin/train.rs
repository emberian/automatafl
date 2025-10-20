use automatafl_learner::*;
use automatafl_fastlogic::Pid;
use burn::backend::ndarray::NdArrayDevice;
use burn_ndarray::NdArray;
use clap::Parser;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, warn};

type Backend = NdArray;

/// AlphaZero-style training for Automatafl
#[derive(Parser, Debug)]
#[command(name = "train")]
#[command(about = "Train neural network to play Automatafl using AlphaZero algorithm")]
struct Args {
    /// Path to configuration file
    #[arg(short, long, default_value = "config.toml")]
    config: PathBuf,

    /// Resume from checkpoint iteration (optional)
    #[arg(short, long)]
    resume: Option<usize>,
}

#[derive(Debug, serde::Deserialize)]
struct Config {
    network: NetworkConfig,
    self_play: SelfPlayConfigToml,
    training: TrainingConfig,
    replay_buffer: ReplayBufferConfig,
    evaluation: EvaluationConfig,
    checkpoint: CheckpointConfig,
    general: GeneralConfig,
}

#[derive(Debug, serde::Deserialize)]
struct NetworkConfig {
    num_blocks: usize,
    num_channels: usize,
    policy_channels: usize,
    value_channels: usize,
}

#[derive(Debug, serde::Deserialize)]
struct SelfPlayConfigToml {
    mcts_simulations: usize,
    temperature_init: f32,
    temperature_final: f32,
    temperature_threshold: usize,
    c_puct: f32,
    num_workers: usize,
    games_per_worker: usize,
}

#[derive(Debug, serde::Deserialize)]
struct ReplayBufferConfig {
    capacity: usize,
}

#[derive(Debug, serde::Deserialize)]
struct EvaluationConfig {
    num_games: usize,
    mcts_simulations: usize,
    accept_threshold: f32,
}

#[derive(Debug, serde::Deserialize)]
struct CheckpointConfig {
    checkpoint_dir: PathBuf,
    keep_last_n: usize,
    save_every: usize,
}

#[derive(Debug, serde::Deserialize)]
struct GeneralConfig {
    num_iterations: usize,
    seed: u64,
}

fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let args = Args::parse();

    // Load configuration
    info!("Loading configuration from {:?}", args.config);
    let config_str = std::fs::read_to_string(&args.config)?;
    let config: Config = toml::from_str(&config_str)?;

    info!("Configuration loaded: {:#?}", config);

    // Initialize device
    let device = NdArrayDevice::Cpu;
    info!("Using device: {:?}", device);

    // Create network config
    let net_config = NetConfig {
        num_blocks: config.network.num_blocks,
        num_channels: config.network.num_channels,
        policy_channels: config.network.policy_channels,
        value_channels: config.network.value_channels,
    };

    // Initialize checkpoint manager
    let checkpoint_manager = CheckpointManager::new(
        config.checkpoint.checkpoint_dir.clone(),
        config.checkpoint.keep_last_n,
    )?;

    // Initialize or resume model and buffer
    let (mut current_model, mut buffer, start_iteration) = if let Some(resume_iter) = args.resume {
        info!("Resuming from checkpoint iteration {}", resume_iter);
        let (model, buffer, metadata) = checkpoint_manager.load(resume_iter, &net_config, &device)?;
        info!("Loaded checkpoint from iteration {}", metadata.iteration);
        (model, buffer, resume_iter + 1)
    } else {
        info!("Starting new training from scratch");
        let model = AutomataflNet::<Backend>::new(&net_config, &device);
        let buffer = ReplayBuffer::new(config.replay_buffer.capacity);
        (model, buffer, 0)
    };

    // Wrap model in Arc for sharing
    let mut best_model = Arc::new(current_model.clone());

    // Self-play configuration
    let self_play_config = SelfPlayConfig {
        mcts_simulations: config.self_play.mcts_simulations,
        temperature_init: config.self_play.temperature_init,
        temperature_final: config.self_play.temperature_final,
        temperature_threshold: config.self_play.temperature_threshold,
        c_puct: config.self_play.c_puct,
    };

    // MCTS configuration for evaluation
    let eval_mcts_config = MCTSConfig {
        num_simulations: config.evaluation.mcts_simulations,
        c_puct: 1.0,
        ..Default::default()
    };

    // Main training loop
    info!("Starting training loop for {} iterations", config.general.num_iterations);

    for iteration in start_iteration..config.general.num_iterations {
        info!("=== Iteration {}/{} ===", iteration + 1, config.general.num_iterations);

        // Phase 1: Self-play
        info!("Running self-play with {} workers, {} games each",
              config.self_play.num_workers, config.self_play.games_per_worker);

        let examples = ParallelSelfPlay::generate_examples(
            best_model.clone(),
            device.clone(),
            self_play_config.clone(),
            config.self_play.num_workers,
            config.self_play.games_per_worker,
            Pid(0),
        );

        info!("Generated {} training examples from self-play", examples.len());

        // Add examples to replay buffer
        buffer.add_examples(examples);
        info!("Replay buffer size: {}/{}", buffer.len(), config.replay_buffer.capacity);

        // Phase 2: Training (placeholder - Trainer needs autodiff implementation)
        info!("Training phase (placeholder - not yet implemented)");
        // TODO: Once Trainer is fully implemented with autodiff:
        // let (updated_model, metrics) = trainer.train_iteration(current_model, &buffer, &device);
        // current_model = updated_model;

        // For now, current_model stays the same (no actual training)
        let candidate_model = Arc::new(current_model.clone());

        // Phase 3: Evaluation
        if iteration > 0 && iteration % 5 == 0 {
            info!("Evaluating new model against best model ({} games)", config.evaluation.num_games);

            let eval_result = Evaluator::evaluate(
                candidate_model.clone(),
                best_model.clone(),
                device.clone(),
                eval_mcts_config.clone(),
                config.evaluation.num_games,
            );

            info!("Evaluation result: Wins={}, Losses={}, Draws={}, Win Rate={:.2}%",
                  eval_result.wins, eval_result.losses, eval_result.draws,
                  eval_result.win_rate() * 100.0);

            if eval_result.should_accept(config.evaluation.accept_threshold) {
                info!("New model accepted! Updating best model.");
                best_model = candidate_model.clone();
                checkpoint_manager.save_best(&(*best_model))?;
            } else {
                warn!("New model rejected. Win rate {:.2}% < threshold {:.2}%",
                      eval_result.win_rate() * 100.0,
                      config.evaluation.accept_threshold * 100.0);
            }
        }

        // Phase 4: Checkpointing
        if (iteration + 1) % config.checkpoint.save_every == 0 {
            info!("Saving checkpoint at iteration {}", iteration);
            checkpoint_manager.save(&(*best_model), &buffer, iteration)?;
            info!("Checkpoint saved");
        }
    }

    info!("Training completed!");
    info!("Best model saved in checkpoint directory");

    Ok(())
}
