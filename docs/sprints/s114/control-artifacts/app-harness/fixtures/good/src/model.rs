use std::collections::BTreeSet;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub id: String,
    pub priority: u8,
    pub state: JobState,
    pub dependencies: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobState {
    Pending,
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DependencyError {
    Empty,
    Duplicate,
    SelfReference,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanError {
    InvalidLine {
        line: usize,
        reason: String,
    },
    DuplicateId {
        id: String,
    },
    InvalidDependency {
        job: String,
        dependency: String,
        kind: DependencyError,
    },
    Cycle {
        remaining: Vec<String>,
    },
}

impl fmt::Display for DependencyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::Empty => "empty",
            Self::Duplicate => "duplicate",
            Self::SelfReference => "self-reference",
            Self::Unknown => "unknown",
        };
        formatter.write_str(message)
    }
}

impl fmt::Display for PlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLine { line, reason } => {
                write!(formatter, "invalid manifest line {line}: {reason}")
            }
            Self::DuplicateId { id } => write!(formatter, "duplicate job ID `{id}`"),
            Self::InvalidDependency {
                job,
                dependency,
                kind,
            } => {
                let shown = if dependency.is_empty() {
                    "<empty>"
                } else {
                    dependency
                };
                write!(formatter, "{kind} dependency `{shown}` for job `{job}`")
            }
            Self::Cycle { remaining } => {
                write!(
                    formatter,
                    "dependency cycle among: {}",
                    remaining.join(", ")
                )
            }
        }
    }
}

impl std::error::Error for PlanError {}

pub(crate) fn validate_jobs(jobs: &[Job]) -> Result<(), PlanError> {
    let mut ids = BTreeSet::new();
    for (index, job) in jobs.iter().enumerate() {
        if job.id.is_empty() || job.id.trim() != job.id {
            return Err(PlanError::InvalidLine {
                line: index + 1,
                reason: "job ID must be non-empty and already trimmed".to_string(),
            });
        }
        if job.priority > 9 {
            return Err(PlanError::InvalidLine {
                line: index + 1,
                reason: "priority must be in 0..=9".to_string(),
            });
        }
        if !ids.insert(job.id.clone()) {
            return Err(PlanError::DuplicateId { id: job.id.clone() });
        }
    }

    for job in jobs {
        let mut dependencies = BTreeSet::new();
        for dependency in &job.dependencies {
            let kind = if dependency.is_empty() || dependency.trim() != dependency {
                Some(DependencyError::Empty)
            } else if dependency == &job.id {
                Some(DependencyError::SelfReference)
            } else if !dependencies.insert(dependency.clone()) {
                Some(DependencyError::Duplicate)
            } else if !ids.contains(dependency) {
                Some(DependencyError::Unknown)
            } else {
                None
            };
            if let Some(kind) = kind {
                return Err(PlanError::InvalidDependency {
                    job: job.id.clone(),
                    dependency: dependency.clone(),
                    kind,
                });
            }
        }
    }
    Ok(())
}
