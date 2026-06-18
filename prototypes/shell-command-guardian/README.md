# shell-command-guardian

A throwaway Oracle Executor prototype for shell command risk scoring.

The default evaluation dataset has 3300 JSONL rows:

- 2400 visible benign commands
- 300 visible malicious commands
- 300 hidden benign script commands
- 300 hidden malicious script commands

Rows are shuffled with a deterministic seed so the corpus is repeatable without putting every
malicious command at a predictable interval. The command templates include routine commands,
ambiguous benign commands, obvious malicious commands, disguised malicious commands, and hidden
script invocations.

The runner never executes a command. It scores each command, applies policy, and reports whether the
policy would allow, ask a human, or deny. The main quality metrics are false negatives, false
positives, and fast-track rate.

## Generate the dataset

```
node scripts/generate-dataset.mjs
node scripts/generate-dataset.mjs --seed 42
node scripts/generate-dataset.mjs --benign 2400 --malicious 300 --hidden-benign 300 --hidden-malicious 300
```

Or, if `try.sh` has your local environment:

```
./try.sh generate --seed 42
```

The same cohort sizes can be set with:

```
SHELL_GUARD_VISIBLE_BENIGN=2400
SHELL_GUARD_VISIBLE_MALICIOUS=300
SHELL_GUARD_HIDDEN_BENIGN=300
SHELL_GUARD_HIDDEN_MALICIOUS=300
```

## Deterministic rules-only run

```
cargo run -- --judge rules --analysis
```

## LM Studio run

The prototype uses the same LM Studio environment names as the other local LLM prototypes.

```
export LOOM_LLM_BASE_URL="http://localhost:1234"
export LOOM_LLM_TOKEN="sk-lm-cL6kSL9o:OwhBkvVACocQnZbNRJqf"
export LOOM_LLM_MODEL="google/gemma-4-31b-qat"
export LOOM_LLM_API="rest-v1"
cargo run -- --judge lm-studio --analysis --limit 100
```

If `try.sh` has your local credentials, use:

```
./try.sh generate --seed 42
./try.sh smoke
./try.sh full
```

`smoke` runs the first 100 shuffled rows. `full` regenerates the default evaluation corpus and runs
all rows with `--quiet`, so the terminal shows the analysis instead of thousands of CSV rows.

With 300 visible malicious and 300 hidden malicious examples, a run with zero false allows gives a
rough 95 percent upper bound of about 1 percent for each malicious cohort.

## Policy

Default thresholds:

- `1..3`: allow
- `4..6`: human review
- `7..10`: deny

Override with:

```
SHELL_GUARD_ALLOW_MAX=3
SHELL_GUARD_DENY_MIN=7
```

For hidden script commands, run with `--script-evidence include` to provide the script source to the
judge. Without script evidence, the executor should escalate rather than guessing.
