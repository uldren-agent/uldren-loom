mod maze;
mod oracle;
mod render;
mod run;

use anyhow::{bail, Context, Result};
use maze::Maze;
use oracle::{DeterministicOracle, LmStudioOracle, OracleKind};
use run::{run_scenario, ExecutionMode, RunConfig, Scenario, Visibility};
use std::env;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse(env::args().skip(1).collect())?;
    let scenarios = build_scenarios(&args);
    let oracle_kind = args.oracle;

    println!(
        "oracle={} execution={} sizes={:?} seed={} path_limit={} max_calls={} max_steps={}",
        oracle_kind.as_str(),
        args.execution.name(),
        args.sizes,
        args.seed,
        args.path_limit,
        args.max_calls,
        args.max_steps
    );
    println!(
        "size,execution,scenario,exit_known,visibility,exited,loops,calls,steps,optimal,efficiency,invalid_responses,empty_paths,invalid_moves,blocked_paths,truncated_paths,cycle_stops,rejected_moves,fallback_moves,revisits,transcript"
    );

    let mut rows = Vec::new();
    for size in args.sizes {
        let maze = Maze::generate(size, size, args.seed)?;
        for scenario in &scenarios {
            let config = RunConfig {
                run_label: format!("{}-{}", oracle_kind.as_str(), args.execution.name()),
                execution: args.execution,
                path_limit: args.path_limit,
                max_calls: args.max_calls,
                max_steps: args.max_steps,
                retries: args.retries,
                max_position_visits: args.max_position_visits,
                transcripts_dir: args.transcripts_dir.clone(),
            };
            let report = match oracle_kind {
                OracleKind::Deterministic => {
                    let mut oracle = DeterministicOracle;
                    run_scenario(&maze, scenario, &config, &mut oracle).await?
                }
                OracleKind::LmStudio => {
                    let mut oracle = LmStudioOracle::from_env()?;
                    run_scenario(&maze, scenario, &config, &mut oracle).await?
                }
            };
            rows.push(SummaryRow {
                size,
                execution: args.execution.name().to_string(),
                scenario: scenario.name(),
                exited: report.exited,
                loops: report.loops,
                oracle_calls: report.oracle_calls,
                steps_taken: report.steps_taken,
                optimal_steps: report.optimal_steps,
                efficiency: report.efficiency(),
                invalid_responses: report.invalid_responses,
                empty_paths: report.empty_paths,
                invalid_moves: report.invalid_moves,
                blocked_paths: report.blocked_paths,
                truncated_paths: report.truncated_paths,
                cycle_stops: report.cycle_stops,
                rejected_moves: report.rejected_moves,
                fallback_moves: report.fallback_moves,
                revisits: report.revisits,
            });
            println!(
                "{},{},{},{},{},{},{},{},{},{},{:.4},{},{},{},{},{},{},{},{},{},{}",
                size,
                args.execution.name(),
                scenario.name(),
                scenario.exit_known,
                scenario.visibility.name(),
                report.exited,
                report.loops,
                report.oracle_calls,
                report.steps_taken,
                report.optimal_steps,
                report.efficiency(),
                report.invalid_responses,
                report.empty_paths,
                report.invalid_moves,
                report.blocked_paths,
                report.truncated_paths,
                report.cycle_stops,
                report.rejected_moves,
                report.fallback_moves,
                report.revisits,
                report.transcript_path.display()
            );
        }
    }

    if args.analysis {
        print_analysis(&rows);
    }

    Ok(())
}

fn build_scenarios(args: &Args) -> Vec<Scenario> {
    let visibilities = match args.visibility {
        Some(v) => vec![v],
        None => vec![
            Visibility::Full,
            Visibility::Percent(50),
            Visibility::Percent(15),
        ],
    };
    let exit_modes = match args.exit_known {
        Some(known) => vec![known],
        None => vec![true, false],
    };

    let mut scenarios = Vec::new();
    for exit_known in exit_modes {
        for visibility in &visibilities {
            scenarios.push(Scenario {
                visibility: *visibility,
                exit_known,
            });
        }
    }
    scenarios
}

struct Args {
    oracle: OracleKind,
    execution: ExecutionMode,
    sizes: Vec<usize>,
    seed: u64,
    path_limit: usize,
    max_calls: usize,
    max_steps: usize,
    retries: usize,
    max_position_visits: usize,
    analysis: bool,
    visibility: Option<Visibility>,
    exit_known: Option<bool>,
    transcripts_dir: PathBuf,
}

impl Args {
    fn parse(raw: Vec<String>) -> Result<Self> {
        let mut oracle = OracleKind::Deterministic;
        let mut execution = ExecutionMode::Permissive;
        let mut sizes = vec![7, 11, 21];
        let mut seed = 7;
        let mut path_limit = 24;
        let mut max_calls = 80;
        let mut max_steps = 5000;
        let mut retries = 1;
        let mut max_position_visits = 8;
        let mut analysis = false;
        let mut visibility = None;
        let mut exit_known = None;
        let mut transcripts_dir = PathBuf::from("transcripts");

        let mut i = 0;
        while i < raw.len() {
            let flag = raw[i].as_str();
            match flag {
                "--oracle" => {
                    oracle = OracleKind::parse(value(&raw, &mut i, flag)?)?;
                }
                "--execution" => {
                    execution = ExecutionMode::parse(value(&raw, &mut i, flag)?)?;
                }
                "--size" => {
                    sizes = vec![parse_size(value(&raw, &mut i, flag)?)?];
                }
                "--sizes" => {
                    sizes = parse_sizes(value(&raw, &mut i, flag)?)?;
                }
                "--seed" => {
                    seed = value(&raw, &mut i, flag)?
                        .parse()
                        .context("invalid --seed")?;
                }
                "--path-limit" => {
                    path_limit = value(&raw, &mut i, flag)?
                        .parse()
                        .context("invalid --path-limit")?;
                    if path_limit == 0 {
                        bail!("--path-limit must be greater than 0");
                    }
                }
                "--max-calls" => {
                    max_calls = value(&raw, &mut i, flag)?
                        .parse()
                        .context("invalid --max-calls")?;
                }
                "--max-steps" => {
                    max_steps = value(&raw, &mut i, flag)?
                        .parse()
                        .context("invalid --max-steps")?;
                }
                "--retries" => {
                    retries = value(&raw, &mut i, flag)?
                        .parse()
                        .context("invalid --retries")?;
                }
                "--max-position-visits" => {
                    max_position_visits = value(&raw, &mut i, flag)?
                        .parse()
                        .context("invalid --max-position-visits")?;
                }
                "--analysis" => {
                    analysis = true;
                }
                "--visibility" => {
                    visibility = Some(Visibility::parse(value(&raw, &mut i, flag)?)?);
                }
                "--exit-known" => {
                    exit_known = Some(true);
                }
                "--exit-hidden" => {
                    exit_known = Some(false);
                }
                "--transcripts-dir" => {
                    transcripts_dir = PathBuf::from(value(&raw, &mut i, flag)?);
                }
                "--help" | "-h" => {
                    print_help();
                    std::process::exit(0);
                }
                other => bail!("unknown argument {other}"),
            }
            i += 1;
        }

        if sizes.is_empty() {
            bail!("at least one size is required");
        }

        Ok(Self {
            oracle,
            execution,
            sizes,
            seed,
            path_limit,
            max_calls,
            max_steps,
            retries,
            max_position_visits,
            analysis,
            visibility,
            exit_known,
            transcripts_dir,
        })
    }
}

fn value(raw: &[String], i: &mut usize, flag: &str) -> Result<String> {
    *i += 1;
    raw.get(*i)
        .cloned()
        .with_context(|| format!("{flag} requires a value"))
}

fn parse_sizes(value: String) -> Result<Vec<usize>> {
    value
        .split(',')
        .map(|part| parse_size(part.trim().to_string()))
        .collect()
}

fn parse_size(value: String) -> Result<usize> {
    let size: usize = value.parse().context("invalid maze size")?;
    if size < 2 {
        bail!("maze size must be at least 2");
    }
    if size >= 7 && size % 2 == 0 {
        bail!("maze sizes 7 and above must be odd");
    }
    Ok(size)
}

fn print_help() {
    println!(
        "maze-oracle

Options:
  --oracle deterministic|lm-studio
  --execution permissive|guarded
  --size N
  --sizes A,B,C
  --seed N
  --path-limit N
  --visibility full|50|15
  --exit-known
  --exit-hidden
  --max-calls N
  --max-steps N
  --retries N
  --max-position-visits N
  --analysis
  --transcripts-dir PATH"
    );
}

struct SummaryRow {
    size: usize,
    execution: String,
    scenario: String,
    exited: bool,
    loops: usize,
    oracle_calls: usize,
    steps_taken: usize,
    optimal_steps: usize,
    efficiency: f64,
    invalid_responses: usize,
    empty_paths: usize,
    invalid_moves: usize,
    blocked_paths: usize,
    truncated_paths: usize,
    cycle_stops: usize,
    rejected_moves: usize,
    fallback_moves: usize,
    revisits: usize,
}

fn print_analysis(rows: &[SummaryRow]) {
    println!();
    println!("analysis");
    println!(
        "size,rank,execution,scenario,exited,efficiency,steps,optimal,loops,calls,mistakes,fallback_moves"
    );

    let mut sizes = rows.iter().map(|row| row.size).collect::<Vec<_>>();
    sizes.sort_unstable();
    sizes.dedup();

    for size in sizes {
        let mut group = rows
            .iter()
            .filter(|row| row.size == size)
            .collect::<Vec<_>>();
        group.sort_by(|a, b| {
            b.exited
                .cmp(&a.exited)
                .then_with(|| b.efficiency.total_cmp(&a.efficiency))
                .then_with(|| a.steps_taken.cmp(&b.steps_taken))
                .then_with(|| a.oracle_calls.cmp(&b.oracle_calls))
        });

        for (idx, row) in group.iter().enumerate() {
            let mistakes = row.invalid_responses
                + row.empty_paths
                + row.invalid_moves
                + row.blocked_paths
                + row.truncated_paths
                + row.cycle_stops
                + row.rejected_moves
                + row.revisits;
            println!(
                "{},{},{},{},{},{:.4},{},{},{},{},{},{}",
                size,
                idx + 1,
                row.execution,
                row.scenario,
                row.exited,
                row.efficiency,
                row.steps_taken,
                row.optimal_steps,
                row.loops,
                row.oracle_calls,
                mistakes,
                row.fallback_moves
            );
        }
    }
}
