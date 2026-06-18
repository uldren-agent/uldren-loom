use crate::maze::{Dir, Maze, Pos};
use crate::oracle::{Oracle, OracleAnswer};
use crate::render::{render_visible, Window};
use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Visibility {
    Full,
    Percent(u8),
}

impl Visibility {
    pub fn parse(s: String) -> Result<Self> {
        match s.as_str() {
            "full" => Ok(Self::Full),
            "50" | "50%" | "view-50" => Ok(Self::Percent(50)),
            "15" | "15%" | "view-15" => Ok(Self::Percent(15)),
            _ => anyhow::bail!("visibility must be full, 50, or 15"),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Percent(50) => "view-50",
            Self::Percent(15) => "view-15",
            Self::Percent(_) => "view-custom",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Scenario {
    pub visibility: Visibility,
    pub exit_known: bool,
}

impl Scenario {
    pub fn name(&self) -> String {
        let exit = if self.exit_known {
            "exit-known"
        } else {
            "exit-hidden"
        };
        format!("{}-{exit}", self.visibility.name())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionMode {
    Permissive,
    Guarded,
}

impl ExecutionMode {
    pub fn parse(s: String) -> Result<Self> {
        match s.as_str() {
            "permissive" => Ok(Self::Permissive),
            "guarded" => Ok(Self::Guarded),
            _ => anyhow::bail!("execution must be permissive or guarded"),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Permissive => "permissive",
            Self::Guarded => "guarded",
        }
    }
}

#[derive(Clone)]
pub struct RunConfig {
    pub run_label: String,
    pub execution: ExecutionMode,
    pub path_limit: usize,
    pub max_calls: usize,
    pub max_steps: usize,
    pub retries: usize,
    pub max_position_visits: usize,
    pub transcripts_dir: PathBuf,
}

#[derive(Debug)]
pub struct RunReport {
    pub exited: bool,
    pub loops: usize,
    pub oracle_calls: usize,
    pub steps_taken: usize,
    pub optimal_steps: usize,
    pub invalid_responses: usize,
    pub empty_paths: usize,
    pub invalid_moves: usize,
    pub blocked_paths: usize,
    pub truncated_paths: usize,
    pub cycle_stops: usize,
    pub rejected_moves: usize,
    pub fallback_moves: usize,
    pub revisits: usize,
    pub transcript_path: PathBuf,
}

impl RunReport {
    pub fn efficiency(&self) -> f64 {
        if !self.exited || self.steps_taken == 0 {
            0.0
        } else {
            self.optimal_steps as f64 / self.steps_taken as f64
        }
    }
}

#[derive(Serialize)]
struct TranscriptEvent<'a> {
    event: &'a str,
    call: usize,
    rat: Pos,
    window: Window,
    visible_digest: String,
    visible: &'a str,
    raw_answer: Option<&'a str>,
    parsed_path: Vec<&'static str>,
    executed: Vec<&'static str>,
    blocked_on: Option<&'static str>,
    rejected: Vec<&'static str>,
    fallback: Option<&'static str>,
    truncated: bool,
    stopped: Option<&'static str>,
    error: Option<String>,
}

pub async fn run_scenario<O: Oracle>(
    maze: &Maze,
    scenario: &Scenario,
    config: &RunConfig,
    oracle: &mut O,
) -> Result<RunReport> {
    fs::create_dir_all(&config.transcripts_dir).context("creating transcripts directory")?;
    let transcript_path = config.transcripts_dir.join(format!(
        "maze-{}x{}-{}-{}.jsonl",
        maze.width(),
        maze.height(),
        config.run_label,
        scenario.name()
    ));
    let mut transcript = File::create(&transcript_path).context("creating transcript")?;
    let optimal_steps = maze
        .shortest_path(maze.entrance())
        .context("maze has no route from entrance to exit")?
        .len();

    let mut rat = maze.entrance();
    let mut seen = HashSet::from([rat]);
    let mut position_visits = HashMap::from([(rat, 1usize)]);
    let mut report = RunReport {
        exited: rat == maze.exit(),
        loops: 0,
        oracle_calls: 0,
        steps_taken: 0,
        optimal_steps,
        invalid_responses: 0,
        empty_paths: 0,
        invalid_moves: 0,
        blocked_paths: 0,
        truncated_paths: 0,
        cycle_stops: 0,
        rejected_moves: 0,
        fallback_moves: 0,
        revisits: 0,
        transcript_path,
    };

    while !report.exited
        && report.oracle_calls < config.max_calls
        && report.steps_taken < config.max_steps
    {
        report.loops += 1;
        let visible = render_visible(maze, rat, scenario.visibility);
        let digest = blake3::hash(visible.ascii.as_bytes()).to_hex().to_string();
        let mut answer = None;
        let mut last_error = None;

        for _ in 0..=config.retries {
            report.oracle_calls += 1;
            match oracle
                .ask(maze, scenario, rat, &visible, config.path_limit)
                .await
            {
                Ok(parsed) => {
                    answer = Some(parsed);
                    break;
                }
                Err(err) => {
                    report.invalid_responses += 1;
                    last_error = Some(format!("{err:#}"));
                }
            }
        }

        let Some(answer) = answer else {
            write_event(
                &mut transcript,
                &TranscriptEvent {
                    event: "invalid-response",
                    call: report.oracle_calls,
                    rat,
                    window: visible.window,
                    visible_digest: digest,
                    visible: &visible.ascii,
                    raw_answer: None,
                    parsed_path: Vec::new(),
                    executed: Vec::new(),
                    blocked_on: None,
                    rejected: Vec::new(),
                    fallback: None,
                    truncated: false,
                    stopped: Some("invalid-response"),
                    error: last_error,
                },
            )?;
            break;
        };

        let mut answer = answer;
        let truncated = answer.path.len() > config.path_limit;
        if truncated {
            report.truncated_paths += 1;
            answer.path.truncate(config.path_limit);
        }

        if answer.path.is_empty() {
            report.empty_paths += 1;
            write_event(
                &mut transcript,
                &TranscriptEvent {
                    event: "empty-path",
                    call: report.oracle_calls,
                    rat,
                    window: visible.window,
                    visible_digest: digest,
                    visible: &visible.ascii,
                    raw_answer: Some(&answer.raw),
                    parsed_path: Vec::new(),
                    executed: Vec::new(),
                    blocked_on: None,
                    rejected: Vec::new(),
                    fallback: None,
                    truncated,
                    stopped: Some("empty-path"),
                    error: None,
                },
            )?;
            continue;
        }

        let outcome = execute_path(
            maze,
            &mut rat,
            &answer,
            &mut report,
            &mut seen,
            &mut position_visits,
            config.execution,
        );
        let stopped = if position_visits
            .get(&rat)
            .is_some_and(|count| *count > config.max_position_visits)
        {
            report.cycle_stops += 1;
            Some("cycle")
        } else {
            None
        };
        write_event(
            &mut transcript,
            &TranscriptEvent {
                event: "oracle-step",
                call: report.oracle_calls,
                rat,
                window: visible.window,
                visible_digest: digest,
                visible: &visible.ascii,
                raw_answer: Some(&answer.raw),
                parsed_path: path_names(&answer.path),
                executed: path_names(&outcome.executed),
                blocked_on: outcome.blocked_on.map(Dir::as_str),
                rejected: path_names(&outcome.rejected),
                fallback: outcome.fallback.map(Dir::as_str),
                truncated,
                stopped,
                error: None,
            },
        )?;

        report.exited = rat == maze.exit();
        if stopped.is_some() {
            break;
        }
    }

    Ok(report)
}

struct ExecuteOutcome {
    executed: Vec<Dir>,
    blocked_on: Option<Dir>,
    rejected: Vec<Dir>,
    fallback: Option<Dir>,
}

fn execute_path(
    maze: &Maze,
    rat: &mut Pos,
    answer: &OracleAnswer,
    report: &mut RunReport,
    seen: &mut HashSet<Pos>,
    position_visits: &mut HashMap<Pos, usize>,
    execution: ExecutionMode,
) -> ExecuteOutcome {
    let mut executed = Vec::new();
    let mut rejected = Vec::new();
    let mut fallback = None;
    for dir in &answer.path {
        if *rat == maze.exit() {
            break;
        }
        match maze.step(*rat, *dir) {
            Some(next) => {
                if config_allows_move(maze, *rat, next, execution) {
                    apply_move(next, rat, report, seen, position_visits);
                    executed.push(*dir);
                } else {
                    report.rejected_moves += 1;
                    rejected.push(*dir);
                    if let Some(best) = best_progress_move(maze, *rat) {
                        if let Some(next) = maze.step(*rat, best) {
                            apply_move(next, rat, report, seen, position_visits);
                            report.fallback_moves += 1;
                            executed.push(best);
                            fallback = Some(best);
                        }
                    }
                    break;
                }
            }
            None => {
                report.invalid_moves += 1;
                report.blocked_paths += 1;
                return ExecuteOutcome {
                    executed,
                    blocked_on: Some(*dir),
                    rejected,
                    fallback,
                };
            }
        }
    }
    ExecuteOutcome {
        executed,
        blocked_on: None,
        rejected,
        fallback,
    }
}

fn config_allows_move(maze: &Maze, from: Pos, to: Pos, execution: ExecutionMode) -> bool {
    match execution {
        ExecutionMode::Permissive => true,
        ExecutionMode::Guarded => {
            let Some(before) = maze.shortest_distance(from) else {
                return false;
            };
            let Some(after) = maze.shortest_distance(to) else {
                return false;
            };
            after < before
        }
    }
}

fn best_progress_move(maze: &Maze, from: Pos) -> Option<Dir> {
    [Dir::N, Dir::E, Dir::S, Dir::W]
        .into_iter()
        .filter_map(|dir| {
            let next = maze.step(from, dir)?;
            let distance = maze.shortest_distance(next)?;
            Some((dir, distance))
        })
        .min_by_key(|(_, distance)| *distance)
        .map(|(dir, _)| dir)
}

fn apply_move(
    next: Pos,
    rat: &mut Pos,
    report: &mut RunReport,
    seen: &mut HashSet<Pos>,
    position_visits: &mut HashMap<Pos, usize>,
) {
    *rat = next;
    report.steps_taken += 1;
    if !seen.insert(next) {
        report.revisits += 1;
    }
    *position_visits.entry(next).or_default() += 1;
}

fn write_event(file: &mut File, event: &TranscriptEvent<'_>) -> Result<()> {
    serde_json::to_writer(&mut *file, event).context("writing transcript event")?;
    file.write_all(b"\n")
        .context("writing transcript newline")?;
    Ok(())
}

fn path_names(path: &[Dir]) -> Vec<&'static str> {
    path.iter().map(|dir| dir.as_str()).collect()
}
