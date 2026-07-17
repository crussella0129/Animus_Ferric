//! `ferric-cron` — agentic cron: schedule periodic *agent* tasks (ADR-066,
//! sprint 75).
//!
//! A workspace's `.ferric/cron/` directory holds one TOML file per job. Each job
//! names a **schedule** (an interval) and a **command** — one of Ferric's own
//! guard-contained operations (`dream`, or a `query` with a prompt). A watcher
//! (`ferric cron watch`) checks which jobs are due and runs them; last-run
//! timestamps live in `.ferric/cron/.state.json`, kept out of the user-authored
//! job files.
//!
//! This crate is the **pure** core: parsing schedules and jobs, computing which
//! jobs are due against an injected "now", and reading/writing state. It runs no
//! processes and reads no clock itself, so it is fully unit-testable. The CLI
//! (`ferric cron`) drives it and performs the actual execution by shelling out to
//! the `ferric` binary — so a scheduled task is always a Ferric subcommand, never
//! an arbitrary shell command (the containment boundary, deliberately narrower
//! than the hooks system's arbitrary scripts).

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CronError {
    #[error(
        "invalid schedule '{0}': expected e.g. `30s`, `15m`, `12h`, `2d`, or `hourly`/`daily`/`weekly`"
    )]
    BadSchedule(String),
    #[error("job '{name}': {reason}")]
    BadJob { name: String, reason: String },
    #[error("io error at {path}: {source}")]
    Io {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("parsing {path}: {source}")]
    Toml {
        path: std::path::PathBuf,
        #[source]
        source: toml::de::Error,
    },
}

/// A recurrence interval, stored in milliseconds. Minimum one second.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Schedule {
    ms: u64,
}

impl Schedule {
    pub fn as_ms(self) -> u64 {
        self.ms
    }
    /// Render back to the compact form (`90m` stays `90m`, not `1h30m`).
    pub fn describe(self) -> String {
        let s = self.ms / 1000;
        if s.is_multiple_of(86_400) {
            format!("{}d", s / 86_400)
        } else if s.is_multiple_of(3_600) {
            format!("{}h", s / 3_600)
        } else if s.is_multiple_of(60) {
            format!("{}m", s / 60)
        } else {
            format!("{s}s")
        }
    }
}

/// Parse a schedule string: a number + unit (`s`/`m`/`h`/`d`), or one of the
/// aliases `hourly` / `daily` / `weekly`. Zero and malformed inputs are refused.
pub fn parse_schedule(input: &str) -> Result<Schedule, CronError> {
    let s = input.trim().to_ascii_lowercase();
    let ms = match s.as_str() {
        "hourly" => 3_600_000,
        "daily" => 86_400_000,
        "weekly" => 604_800_000,
        _ => {
            let (num, unit) = s.split_at(
                s.find(|c: char| !c.is_ascii_digit())
                    .ok_or_else(|| CronError::BadSchedule(input.to_string()))?,
            );
            let n: u64 = num
                .parse()
                .map_err(|_| CronError::BadSchedule(input.to_string()))?;
            let unit_ms = match unit {
                "s" => 1_000,
                "m" => 60_000,
                "h" => 3_600_000,
                "d" => 86_400_000,
                _ => return Err(CronError::BadSchedule(input.to_string())),
            };
            n.checked_mul(unit_ms)
                .ok_or_else(|| CronError::BadSchedule(input.to_string()))?
        }
    };
    if ms == 0 {
        return Err(CronError::BadSchedule(input.to_string()));
    }
    Ok(Schedule { ms })
}

/// What a job runs — one of Ferric's own contained operations. Not an arbitrary
/// shell command: the surface is intentionally bounded to the agent operations
/// Ferric already guards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobCommand {
    /// `ferric dream` — consolidate recent traces into MEMORY.md.
    Dream,
    /// `ferric query <prompt>` — one workspace-scoped agent run.
    Query {
        prompt: String,
        /// Run against the built-in mock (offline) — for testing a schedule
        /// without a live model.
        mock: bool,
    },
}

impl JobCommand {
    pub fn kind(&self) -> &'static str {
        match self {
            JobCommand::Dream => "dream",
            JobCommand::Query { .. } => "query",
        }
    }
}

/// A fully-validated cron job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CronJob {
    pub name: String,
    pub schedule: Schedule,
    pub command: JobCommand,
    pub enabled: bool,
}

/// The on-disk TOML shape (before validation).
#[derive(Debug, Deserialize)]
struct RawJob {
    schedule: String,
    command: String,
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    mock: bool,
    #[serde(default = "default_true")]
    enabled: bool,
}

fn default_true() -> bool {
    true
}

/// Parse and validate one job's TOML text under a given `name` (the filename
/// stem). A `query` command requires a non-empty `prompt`.
pub fn parse_job(name: &str, toml_str: &str) -> Result<CronJob, CronError> {
    let raw: RawJob = toml::from_str(toml_str).map_err(|source| CronError::Toml {
        path: std::path::PathBuf::from(format!("{name}.toml")),
        source,
    })?;
    let schedule = parse_schedule(&raw.schedule)?;
    let command = match raw.command.trim().to_ascii_lowercase().as_str() {
        "dream" => JobCommand::Dream,
        "query" => {
            let prompt = raw.prompt.unwrap_or_default();
            if prompt.trim().is_empty() {
                return Err(CronError::BadJob {
                    name: name.to_string(),
                    reason: "command = \"query\" requires a non-empty `prompt`".to_string(),
                });
            }
            JobCommand::Query {
                prompt,
                mock: raw.mock,
            }
        }
        other => {
            return Err(CronError::BadJob {
                name: name.to_string(),
                reason: format!("unknown command '{other}' (expected `dream` or `query`)"),
            });
        }
    };
    Ok(CronJob {
        name: name.to_string(),
        schedule,
        command,
        enabled: raw.enabled,
    })
}

/// Load every job from a `.ferric/cron/` directory: each `*.toml` file (the
/// filename stem is the job name), sorted by name for determinism. The
/// `.state.json` file and non-toml entries are skipped. A missing directory
/// yields an empty list (no jobs configured).
pub fn load_jobs(dir: &Path) -> Result<Vec<CronJob>, CronError> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .map_err(|source| CronError::Io {
            path: dir.to_path_buf(),
            source,
        })?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("toml"))
        .collect();
    files.sort();

    let mut jobs = Vec::with_capacity(files.len());
    for path in files {
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("job")
            .to_string();
        let text = std::fs::read_to_string(&path).map_err(|source| CronError::Io {
            path: path.clone(),
            source,
        })?;
        jobs.push(parse_job(&name, &text)?);
    }
    Ok(jobs)
}

/// Last-run timestamps (epoch ms), keyed by job name.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CronState {
    #[serde(default)]
    pub last_run: BTreeMap<String, u64>,
}

impl CronState {
    pub fn last_run(&self, name: &str) -> Option<u64> {
        self.last_run.get(name).copied()
    }
    pub fn mark_run(&mut self, name: &str, now_ms: u64) {
        self.last_run.insert(name.to_string(), now_ms);
    }

    /// Load state from `path`; a missing/blank/corrupt file is treated as empty
    /// state (state is a runtime cache, never a hard dependency).
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<(), CronError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| CronError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let text = serde_json::to_string_pretty(self).unwrap_or_default();
        std::fs::write(path, text).map_err(|source| CronError::Io {
            path: path.to_path_buf(),
            source,
        })
    }
}

/// Is `job` due at `now_ms`, given its `last_run`? A disabled job is never due;
/// a never-run job is due immediately; otherwise it is due once a full interval
/// has elapsed since the last run.
pub fn is_due(job: &CronJob, last_run: Option<u64>, now_ms: u64) -> bool {
    if !job.enabled {
        return false;
    }
    match last_run {
        None => true,
        Some(t) => now_ms.saturating_sub(t) >= job.schedule.as_ms(),
    }
}

/// The epoch-ms instant a job next becomes due (`last_run + interval`), or
/// `None` if it has never run (due now).
pub fn next_due_ms(job: &CronJob, last_run: Option<u64>) -> Option<u64> {
    last_run.map(|t| t.saturating_add(job.schedule.as_ms()))
}

/// All currently-due jobs, in the given order.
pub fn due_jobs<'a>(jobs: &'a [CronJob], state: &CronState, now_ms: u64) -> Vec<&'a CronJob> {
    jobs.iter()
        .filter(|j| is_due(j, state.last_run(&j.name), now_ms))
        .collect()
}

/// Render a job-file TOML skeleton for `ferric cron add`.
pub fn job_toml(schedule: &str, command: &JobCommand) -> String {
    let mut s = format!(
        "schedule = \"{schedule}\"\ncommand = \"{}\"\n",
        command.kind()
    );
    if let JobCommand::Query { prompt, mock } = command {
        s.push_str(&format!("prompt = {}\n", toml_escape(prompt)));
        if *mock {
            s.push_str("mock = true\n");
        }
    }
    s.push_str("enabled = true\n");
    s
}

fn toml_escape(s: &str) -> String {
    // Basic TOML string quoting: escape backslashes and quotes.
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(name: &str, sched: &str, enabled: bool) -> CronJob {
        CronJob {
            name: name.to_string(),
            schedule: parse_schedule(sched).unwrap(),
            command: JobCommand::Dream,
            enabled,
        }
    }

    #[test]
    fn schedule_units_and_aliases() {
        assert_eq!(parse_schedule("30s").unwrap().as_ms(), 30_000);
        assert_eq!(parse_schedule("15m").unwrap().as_ms(), 900_000);
        assert_eq!(parse_schedule("12h").unwrap().as_ms(), 43_200_000);
        assert_eq!(parse_schedule("2d").unwrap().as_ms(), 172_800_000);
        assert_eq!(parse_schedule("hourly").unwrap().as_ms(), 3_600_000);
        assert_eq!(parse_schedule("DAILY").unwrap().as_ms(), 86_400_000);
        assert_eq!(parse_schedule("weekly").unwrap().as_ms(), 604_800_000);
    }

    #[test]
    fn schedule_rejects_garbage_and_zero() {
        assert!(parse_schedule("").is_err());
        assert!(parse_schedule("12").is_err()); // no unit
        assert!(parse_schedule("12x").is_err()); // bad unit
        assert!(parse_schedule("0h").is_err()); // zero
        assert!(parse_schedule("abc").is_err());
    }

    #[test]
    fn schedule_describe_roundtrips_compactly() {
        assert_eq!(parse_schedule("12h").unwrap().describe(), "12h");
        assert_eq!(parse_schedule("90m").unwrap().describe(), "90m");
        assert_eq!(parse_schedule("2d").unwrap().describe(), "2d");
        assert_eq!(parse_schedule("45s").unwrap().describe(), "45s");
    }

    #[test]
    fn due_logic() {
        let j = job("d", "1h", true);
        // Never run -> due now.
        assert!(is_due(&j, None, 1_000));
        // Run 30m ago -> not due (needs 1h).
        assert!(!is_due(&j, Some(0), 1_800_000));
        // Run exactly 1h ago -> due.
        assert!(is_due(&j, Some(0), 3_600_000));
        // Disabled -> never due, even if long overdue.
        let off = job("d", "1h", false);
        assert!(!is_due(&off, Some(0), 999_999_999));
    }

    #[test]
    fn next_due_is_last_plus_interval() {
        let j = job("d", "1h", true);
        assert_eq!(next_due_ms(&j, Some(1_000)), Some(3_601_000));
        assert_eq!(next_due_ms(&j, None), None);
    }

    #[test]
    fn due_jobs_filters_by_state() {
        let jobs = vec![job("a", "1h", true), job("b", "1h", true)];
        let mut state = CronState::default();
        state.mark_run("a", 0); // a ran at t=0
        // At t=30m: a not due (ran 30m ago), b due (never run).
        let due = due_jobs(&jobs, &state, 1_800_000);
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].name, "b");
    }

    #[test]
    fn parse_dream_and_query_jobs() {
        let d = parse_job("nightly", "schedule = \"12h\"\ncommand = \"dream\"\n").unwrap();
        assert_eq!(d.command, JobCommand::Dream);
        assert!(d.enabled);

        let q = parse_job(
            "summary",
            "schedule = \"1d\"\ncommand = \"query\"\nprompt = \"summarize\"\nmock = true\n",
        )
        .unwrap();
        assert_eq!(
            q.command,
            JobCommand::Query {
                prompt: "summarize".into(),
                mock: true
            }
        );
    }

    #[test]
    fn query_without_prompt_is_rejected() {
        let err = parse_job("x", "schedule = \"1h\"\ncommand = \"query\"\n").unwrap_err();
        assert!(matches!(err, CronError::BadJob { .. }));
    }

    #[test]
    fn unknown_command_is_rejected() {
        let err = parse_job("x", "schedule = \"1h\"\ncommand = \"rm -rf\"\n").unwrap_err();
        assert!(matches!(err, CronError::BadJob { .. }));
    }

    #[test]
    fn state_roundtrips_and_generated_toml_parses() {
        let mut st = CronState::default();
        st.mark_run("a", 12345);
        let json = serde_json::to_string(&st).unwrap();
        let back: CronState = serde_json::from_str(&json).unwrap();
        assert_eq!(back.last_run("a"), Some(12345));

        // A generated job TOML must parse back (add -> load round-trip).
        let toml = job_toml(
            "6h",
            &JobCommand::Query {
                prompt: "say \"hi\"".into(),
                mock: false,
            },
        );
        let parsed = parse_job("gen", &toml).unwrap();
        assert_eq!(
            parsed.command,
            JobCommand::Query {
                prompt: "say \"hi\"".into(),
                mock: false
            }
        );
    }
}
