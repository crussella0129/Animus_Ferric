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
        // Boxed: `toml::de::Error` is large (>128 bytes on some targets), which
        // otherwise trips clippy's `result_large_err` on every `Result<_,
        // CronError>` (a Windows-CI-only failure — the type is smaller on Linux).
        #[source]
        source: Box<toml::de::Error>,
    },
}

/// A job's schedule: either a fixed recurrence interval, or a **calendar cron
/// expression** (5-field, evaluated in **UTC**).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Schedule {
    /// A fixed interval in milliseconds (minimum one second).
    Interval(u64),
    /// A 5-field cron expression: minute hour day-of-month month day-of-week.
    Cron(CronExpr),
}

impl Schedule {
    /// The interval in ms for an interval schedule, or `None` for a cron one.
    pub fn interval_ms(&self) -> Option<u64> {
        match self {
            Schedule::Interval(ms) => Some(*ms),
            Schedule::Cron(_) => None,
        }
    }
    /// A compact, round-trippable description (`12h`, or `cron(0 2 * * *)`).
    pub fn describe(&self) -> String {
        match self {
            Schedule::Interval(ms) => describe_interval(*ms),
            Schedule::Cron(expr) => format!("cron({})", expr.source),
        }
    }
}

fn describe_interval(ms: u64) -> String {
    let s = ms / 1000;
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

/// Parse a job schedule. A string with five whitespace-separated fields is a
/// cron expression (`0 2 * * *`); anything else is a recurrence interval
/// (`12h`, `daily`, …).
pub fn parse_schedule(input: &str) -> Result<Schedule, CronError> {
    let trimmed = input.trim();
    if trimmed.split_whitespace().count() == 5 {
        Ok(Schedule::Cron(CronExpr::parse(trimmed)?))
    } else {
        Ok(Schedule::Interval(parse_interval_ms(trimmed)?))
    }
}

/// Parse a recurrence interval (`30s`/`15m`/`12h`/`2d`, or
/// `hourly`/`daily`/`weekly`) to milliseconds. Also used for the `cron watch`
/// tick interval, which is always an interval (never a cron expression).
pub fn parse_interval_ms(input: &str) -> Result<u64, CronError> {
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
    Ok(ms)
}

/// A parsed 5-field cron expression, evaluated in **UTC**. Fields: minute
/// (0–59), hour (0–23), day-of-month (1–31), month (1–12), and day-of-week
/// (0–6, 0 = Sunday; `7` is accepted as Sunday too). Each field supports `*`,
/// a number, a range (`1-5`), a list (`1,3,5`), and a step (`*/15`, `0-30/10`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CronExpr {
    source: String,
    minute: Vec<u32>,
    hour: Vec<u32>,
    dom: Vec<u32>,
    month: Vec<u32>,
    dow: Vec<u32>,
    dom_restricted: bool,
    dow_restricted: bool,
}

impl CronExpr {
    pub fn parse(input: &str) -> Result<CronExpr, CronError> {
        let f: Vec<&str> = input.split_whitespace().collect();
        if f.len() != 5 {
            return Err(CronError::BadSchedule(input.to_string()));
        }
        let bad = || CronError::BadSchedule(input.to_string());
        let minute = parse_field(f[0], 0, 59).ok_or_else(bad)?;
        let hour = parse_field(f[1], 0, 23).ok_or_else(bad)?;
        let dom = parse_field(f[2], 1, 31).ok_or_else(bad)?;
        let month = parse_field(f[3], 1, 12).ok_or_else(bad)?;
        // Day-of-week allows 0–7 (both 0 and 7 mean Sunday); normalize 7 → 0.
        let mut dow = parse_field(f[4], 0, 7).ok_or_else(bad)?;
        for v in &mut dow {
            if *v == 7 {
                *v = 0;
            }
        }
        dow.sort_unstable();
        dow.dedup();
        Ok(CronExpr {
            source: input.to_string(),
            minute,
            hour,
            dom,
            month,
            dow,
            dom_restricted: f[2] != "*",
            dow_restricted: f[4] != "*",
        })
    }

    /// Does the expression fire at this UTC civil time?
    fn matches(&self, min: u32, hour: u32, dom: u32, month: u32, dow: u32) -> bool {
        if !self.minute.contains(&min) || !self.hour.contains(&hour) || !self.month.contains(&month)
        {
            return false;
        }
        let dom_ok = self.dom.contains(&dom);
        let dow_ok = self.dow.contains(&dow);
        // Vixie-cron rule: when BOTH day-of-month and day-of-week are restricted,
        // the job fires when EITHER matches; otherwise both must match.
        if self.dom_restricted && self.dow_restricted {
            dom_ok || dow_ok
        } else {
            dom_ok && dow_ok
        }
    }

    fn matches_ms(&self, now_ms: u64) -> bool {
        let (min, hour, dom, month, dow) = civil_utc(now_ms);
        self.matches(min, hour, dom, month, dow)
    }
}

/// Decompose an epoch-ms instant into UTC (minute, hour, day-of-month, month,
/// day-of-week) with day-of-week 0 = Sunday.
fn civil_utc(now_ms: u64) -> (u32, u32, u32, u32, u32) {
    use chrono::{DateTime, Datelike, Timelike, Utc};
    let dt = DateTime::<Utc>::from_timestamp_millis(now_ms as i64)
        .unwrap_or_else(|| DateTime::<Utc>::from_timestamp(0, 0).unwrap());
    (
        dt.minute(),
        dt.hour(),
        dt.day(),
        dt.month(),
        dt.weekday().num_days_from_sunday(),
    )
}

/// Parse one cron field into the explicit set of values it allows, bounded to
/// `[lo, hi]`. Returns `None` on any malformed or out-of-range input.
fn parse_field(spec: &str, lo: u32, hi: u32) -> Option<Vec<u32>> {
    let mut out: Vec<u32> = Vec::new();
    for part in spec.split(',') {
        // Optional `/step` suffix.
        let (range_part, step) = match part.split_once('/') {
            Some((r, s)) => (r, Some(s.parse::<u32>().ok().filter(|n| *n >= 1)?)),
            None => (part, None),
        };
        let (start, end) = if range_part == "*" {
            (lo, hi)
        } else if let Some((a, b)) = range_part.split_once('-') {
            (a.parse().ok()?, b.parse().ok()?)
        } else {
            let n: u32 = range_part.parse().ok()?;
            (n, n)
        };
        if start > end || start < lo || end > hi {
            return None;
        }
        let step = step.unwrap_or(1);
        let mut v = start;
        while v <= end {
            out.push(v);
            v += step;
        }
    }
    if out.is_empty() {
        return None;
    }
    out.sort_unstable();
    out.dedup();
    Some(out)
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
        source: Box::new(source),
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

/// Minutes to scan forward when computing a cron job's next fire (≈ 366 days),
/// so an annual expression still resolves.
const CRON_SCAN_MINUTES: u64 = 366 * 24 * 60;

/// Is `job` due at `now_ms`, given its `last_run`? A disabled job is never due.
/// For an **interval** schedule: due once a full interval has elapsed (a
/// never-run job is due immediately). For a **cron** schedule: due when the
/// current UTC minute matches the expression and the job has not already fired
/// during this minute.
pub fn is_due(job: &CronJob, last_run: Option<u64>, now_ms: u64) -> bool {
    if !job.enabled {
        return false;
    }
    match &job.schedule {
        Schedule::Interval(ms) => match last_run {
            None => true,
            Some(t) => now_ms.saturating_sub(t) >= *ms,
        },
        Schedule::Cron(expr) => {
            if !expr.matches_ms(now_ms) {
                return false;
            }
            // Fire at most once per matching minute: skip if we already ran
            // within the current minute window.
            let minute_start = now_ms - (now_ms % 60_000);
            last_run.is_none_or(|t| t < minute_start)
        }
    }
}

/// The epoch-ms instant a job next becomes due. For an interval schedule this is
/// `last_run + interval` (`None` if never run — i.e. due now). For a cron
/// schedule it is the next matching UTC minute after `now_ms` (bounded scan;
/// `None` if none within ~366 days).
pub fn next_due_ms(job: &CronJob, last_run: Option<u64>, now_ms: u64) -> Option<u64> {
    match &job.schedule {
        Schedule::Interval(ms) => last_run.map(|t| t.saturating_add(*ms)),
        Schedule::Cron(expr) => {
            let mut t = (now_ms / 60_000 + 1) * 60_000; // next minute boundary
            for _ in 0..CRON_SCAN_MINUTES {
                if expr.matches_ms(t) {
                    return Some(t);
                }
                t += 60_000;
            }
            None
        }
    }
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
        assert_eq!(parse_schedule("30s").unwrap().interval_ms(), Some(30_000));
        assert_eq!(parse_schedule("15m").unwrap().interval_ms(), Some(900_000));
        assert_eq!(
            parse_schedule("12h").unwrap().interval_ms(),
            Some(43_200_000)
        );
        assert_eq!(
            parse_schedule("2d").unwrap().interval_ms(),
            Some(172_800_000)
        );
        assert_eq!(
            parse_schedule("hourly").unwrap().interval_ms(),
            Some(3_600_000)
        );
        assert_eq!(
            parse_schedule("DAILY").unwrap().interval_ms(),
            Some(86_400_000)
        );
        assert_eq!(
            parse_schedule("weekly").unwrap().interval_ms(),
            Some(604_800_000)
        );
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
        assert_eq!(next_due_ms(&j, Some(1_000), 0), Some(3_601_000));
        assert_eq!(next_due_ms(&j, None, 0), None);
    }

    // ── Calendar cron expressions (sprint 76) ──────────────────────────────

    /// 2026-07-01 00:00:00 UTC (a Wednesday) in epoch ms — the anchor for the
    /// cron tests below.
    const WED_2026_07_01_0000: u64 = 1_782_864_000_000;

    fn cron(expr: &str) -> CronJob {
        CronJob {
            name: "c".into(),
            schedule: parse_schedule(expr).unwrap(),
            command: JobCommand::Dream,
            enabled: true,
        }
    }

    #[test]
    fn cron_expression_is_detected_and_described() {
        let s = parse_schedule("0 2 * * *").unwrap();
        assert!(matches!(s, Schedule::Cron(_)));
        assert_eq!(s.interval_ms(), None);
        assert_eq!(s.describe(), "cron(0 2 * * *)");
    }

    #[test]
    fn cron_fields_reject_bad_input() {
        assert!(parse_schedule("60 * * * *").is_err()); // minute out of range
        assert!(parse_schedule("* 24 * * *").is_err()); // hour out of range
        assert!(parse_schedule("* * 0 * *").is_err()); // day-of-month < 1
        assert!(parse_schedule("* * * 13 *").is_err()); // month > 12
        assert!(parse_schedule("* * * * 8").is_err()); // dow > 7
        assert!(parse_schedule("a b c d e").is_err()); // non-numeric
    }

    #[test]
    fn cron_daily_at_0200_fires_only_in_that_minute() {
        let j = cron("0 2 * * *"); // 02:00 UTC daily
        let base = WED_2026_07_01_0000;
        let at_0200 = base + 2 * 3_600_000;
        // Fires at exactly 02:00, having never run.
        assert!(is_due(&j, None, at_0200));
        // Not at 01:59 or 02:01.
        assert!(!is_due(&j, None, at_0200 - 60_000));
        assert!(!is_due(&j, None, at_0200 + 60_000));
        // Once fired at 02:00:00, it will not re-fire later in the same minute.
        assert!(!is_due(&j, Some(at_0200), at_0200 + 30_000));
        // But it is due again the next day at 02:00.
        assert!(is_due(&j, Some(at_0200), at_0200 + 86_400_000));
    }

    #[test]
    fn cron_weekday_range_matches_only_mon_to_fri() {
        let j = cron("0 9 * * 1-5"); // 09:00 UTC, Mon–Fri
        let wed_0900 = WED_2026_07_01_0000 + 9 * 3_600_000; // Wed
        assert!(is_due(&j, None, wed_0900));
        // Two days later is Friday 09:00 → due.
        assert!(is_due(&j, None, wed_0900 + 2 * 86_400_000));
        // Three days later is Saturday 09:00 → NOT due.
        assert!(!is_due(&j, None, wed_0900 + 3 * 86_400_000));
    }

    #[test]
    fn cron_step_and_list_fields() {
        // Every 15 minutes.
        let j = cron("*/15 * * * *");
        let base = WED_2026_07_01_0000;
        assert!(is_due(&j, None, base)); // :00
        assert!(is_due(&j, None, base + 15 * 60_000)); // :15
        assert!(!is_due(&j, None, base + 7 * 60_000)); // :07
        // A comma list of hours.
        let h = cron("0 0,12 * * *");
        assert!(is_due(&h, None, base)); // 00:00
        assert!(is_due(&h, None, base + 12 * 3_600_000)); // 12:00
        assert!(!is_due(&h, None, base + 6 * 3_600_000)); // 06:00
    }

    #[test]
    fn cron_next_due_finds_the_following_fire() {
        let j = cron("0 2 * * *");
        let base = WED_2026_07_01_0000; // 00:00
        // Next fire after midnight is 02:00 the same day.
        assert_eq!(next_due_ms(&j, None, base), Some(base + 2 * 3_600_000));
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

#[cfg(test)]
mod size_guard {
    /// Regression guard for the Windows-CI `result_large_err` failure (sprint 76):
    /// keep `CronError` comfortably under clippy's 128-byte threshold.
    #[test]
    fn cron_error_stays_small() {
        assert!(
            std::mem::size_of::<super::CronError>() <= 96,
            "CronError grew to {} bytes; box a large field to stay under clippy's result_large_err threshold",
            std::mem::size_of::<super::CronError>()
        );
    }
}
