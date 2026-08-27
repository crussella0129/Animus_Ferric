use crate::model::{DependencyError, Job, JobState, PlanError, validate_jobs};

pub fn parse_manifest(input: &str) -> Result<Vec<Job>, PlanError> {
    let mut jobs = Vec::new();

    for (index, raw_line) in input.lines().enumerate() {
        let line_number = index + 1;
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let fields: Vec<&str> = line.split('|').map(str::trim).collect();
        if fields.len() != 4 {
            return Err(invalid_line(line_number, "expected exactly four fields"));
        }

        let id = fields[0];
        if id.is_empty() {
            return Err(invalid_line(line_number, "job ID must not be empty"));
        }
        let priority = fields[1]
            .parse::<u8>()
            .ok()
            .filter(|value| *value <= 9)
            .ok_or_else(|| invalid_line(line_number, "priority must be an integer in 0..=9"))?;
        let state = match fields[2] {
            "pending" => JobState::Pending,
            "done" => JobState::Done,
            _ => {
                return Err(invalid_line(
                    line_number,
                    "state must be exactly `pending` or `done`",
                ));
            }
        };

        let dependencies = if fields[3].is_empty() {
            Vec::new()
        } else {
            let mut parsed = Vec::new();
            for dependency in fields[3].split(',').map(str::trim) {
                if dependency.is_empty() {
                    return Err(PlanError::InvalidDependency {
                        job: id.to_string(),
                        dependency: String::new(),
                        kind: DependencyError::Empty,
                    });
                }
                parsed.push(dependency.to_string());
            }
            parsed
        };

        jobs.push(Job {
            id: id.to_string(),
            priority,
            state,
            dependencies,
        });
    }

    validate_jobs(&jobs)?;
    Ok(jobs)
}

fn invalid_line(line: usize, reason: &str) -> PlanError {
    PlanError::InvalidLine {
        line,
        reason: reason.to_string(),
    }
}
