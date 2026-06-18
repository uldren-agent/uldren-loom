mod dataset;
mod judge;
mod policy;
mod rules;

use anyhow::{bail, Context, Result};
use dataset::CommandCase;
use judge::{rules_result, JudgeKind, LmStudioJudge};
use policy::{Decision, Metrics, Policy};
use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse(env::args().skip(1).collect())?;
    let cases = dataset::load(&args.input)?;
    fs::create_dir_all(&args.results_dir).context("creating results directory")?;
    let output_path = args
        .results_dir
        .join(format!("{}-results.jsonl", args.judge.as_str()));
    let mut output = File::create(&output_path).context("creating results file")?;
    let policy = Policy::from_env();
    let mut metrics = Metrics::default();
    let lm = match args.judge {
        JudgeKind::Rules => None,
        JudgeKind::LmStudio => Some(LmStudioJudge::from_env()?),
    };

    println!(
        "judge={} input={} script_evidence={} limit={:?}",
        args.judge.as_str(),
        args.input.display(),
        args.script_evidence.as_str(),
        args.limit
    );
    if !args.quiet {
        println!("id,label,hidden_script,score,decision,category,command");
    }

    for case in cases.iter().take(args.limit.unwrap_or(usize::MAX)) {
        let include_script = args.script_evidence.include(case.hidden_script);
        let rule_score = rules::score(case, include_script);
        let judged = match (&args.judge, &lm) {
            (JudgeKind::Rules, _) => rules_result(&rule_score),
            (JudgeKind::LmStudio, Some(lm)) => lm.judge(case, &rule_score, include_script).await?,
            _ => unreachable!("LM judge must be initialized"),
        };
        let mut score = judged.score.max(rule_score.score);
        if case.hidden_script && !include_script {
            score = score.max(4);
        }
        let decision = policy.decide(score);
        metrics.observe(case.label, case.hidden_script, decision);
        write_result(&mut output, case, score, decision, &judged.rationale)?;
        if !args.quiet {
            println!(
                "{},{:?},{},{},{},{},{}",
                case.id,
                case.label,
                case.hidden_script,
                score,
                decision.as_str(),
                case.category,
                case.command
            );
        }
    }

    if args.analysis {
        print_analysis(&metrics);
    }
    println!("results={}", output_path.display());
    Ok(())
}

fn write_result(
    output: &mut File,
    case: &CommandCase,
    score: u8,
    decision: Decision,
    rationale: &str,
) -> Result<()> {
    let row = serde_json::json!({
        "id": case.id,
        "command": case.command,
        "label": case.label,
        "category": case.category,
        "hidden_script": case.hidden_script,
        "score": score,
        "decision": decision.as_str(),
        "rationale": rationale,
    });
    serde_json::to_writer(&mut *output, &row).context("writing result")?;
    output.write_all(b"\n").context("writing result newline")?;
    Ok(())
}

fn print_analysis(metrics: &Metrics) {
    println!();
    println!("analysis");
    println!("total={}", metrics.total);
    println!("benign={}", metrics.benign);
    println!("malicious={}", metrics.malicious);
    println!("allow={}", metrics.allow);
    println!("review={}", metrics.review);
    println!("deny={}", metrics.deny);
    println!("false_allow_malicious={}", metrics.false_allow);
    println!(
        "false_review_or_deny_benign={}",
        metrics.false_review_or_deny
    );
    println!(
        "false_negative_rate={:.4}",
        rate(metrics.false_allow, metrics.malicious)
    );
    println!(
        "false_positive_rate={:.4}",
        rate(metrics.false_review_or_deny, metrics.benign)
    );
    println!("fast_track_rate={:.4}", rate(metrics.allow, metrics.total));
    println!("hidden_script={}", metrics.hidden_script);
    println!("hidden_benign={}", metrics.hidden_benign);
    println!("hidden_malicious={}", metrics.hidden_malicious);
    println!("hidden_script_allow={}", metrics.hidden_allow);
    println!("hidden_script_review={}", metrics.hidden_script_review);
    println!("hidden_script_deny={}", metrics.hidden_deny);
    println!(
        "hidden_false_allow_malicious={}",
        metrics.hidden_false_allow
    );
    println!(
        "hidden_false_review_or_deny_benign={}",
        metrics.hidden_false_review_or_deny
    );
    println!(
        "hidden_false_negative_rate={:.4}",
        rate(metrics.hidden_false_allow, metrics.hidden_malicious)
    );
    println!(
        "hidden_false_positive_rate={:.4}",
        rate(metrics.hidden_false_review_or_deny, metrics.hidden_benign)
    );
    println!(
        "hidden_fast_track_rate={:.4}",
        rate(metrics.hidden_allow, metrics.hidden_script)
    );
}

fn rate(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

struct Args {
    judge: JudgeKind,
    input: PathBuf,
    results_dir: PathBuf,
    limit: Option<usize>,
    script_evidence: ScriptEvidence,
    analysis: bool,
    quiet: bool,
}

impl Args {
    fn parse(raw: Vec<String>) -> Result<Self> {
        let mut judge = JudgeKind::Rules;
        let mut input = PathBuf::from("data/commands.jsonl");
        let mut results_dir = PathBuf::from("results");
        let mut limit = None;
        let mut script_evidence = ScriptEvidence::Missing;
        let mut analysis = false;
        let mut quiet = false;

        let mut i = 0;
        while i < raw.len() {
            match raw[i].as_str() {
                "--judge" => judge = JudgeKind::parse(&value(&raw, &mut i, "--judge")?)?,
                "--input" => input = PathBuf::from(value(&raw, &mut i, "--input")?),
                "--results-dir" => {
                    results_dir = PathBuf::from(value(&raw, &mut i, "--results-dir")?);
                }
                "--limit" => {
                    limit = Some(
                        value(&raw, &mut i, "--limit")?
                            .parse()
                            .context("invalid --limit")?,
                    );
                }
                "--script-evidence" => {
                    script_evidence =
                        ScriptEvidence::parse(&value(&raw, &mut i, "--script-evidence")?)?;
                }
                "--analysis" => analysis = true,
                "--quiet" => quiet = true,
                "--help" | "-h" => {
                    print_help();
                    std::process::exit(0);
                }
                other => bail!("unknown argument {other}"),
            }
            i += 1;
        }

        Ok(Self {
            judge,
            input,
            results_dir,
            limit,
            script_evidence,
            analysis,
            quiet,
        })
    }
}

#[derive(Clone, Copy)]
enum ScriptEvidence {
    Missing,
    Include,
}

impl ScriptEvidence {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "missing" => Ok(Self::Missing),
            "include" => Ok(Self::Include),
            _ => bail!("script evidence must be missing or include"),
        }
    }

    fn include(self, hidden_script: bool) -> bool {
        matches!(self, Self::Include) && hidden_script
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Include => "include",
        }
    }
}

fn value(raw: &[String], i: &mut usize, flag: &str) -> Result<String> {
    *i += 1;
    raw.get(*i)
        .cloned()
        .with_context(|| format!("{flag} requires a value"))
}

fn print_help() {
    println!(
        "shell-command-guardian

Options:
  --judge rules|lm-studio
  --input PATH
  --results-dir PATH
  --limit N
  --script-evidence missing|include
  --analysis
  --quiet"
    );
}
