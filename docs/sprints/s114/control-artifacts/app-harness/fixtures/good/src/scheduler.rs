use std::collections::BTreeSet;

use crate::model::{Job, JobState, PlanError, validate_jobs};

pub fn build_plan(jobs: &[Job]) -> Result<Vec<String>, PlanError> {
    validate_jobs(jobs)?;

    let mut satisfied: BTreeSet<String> = jobs
        .iter()
        .filter(|job| job.state == JobState::Done)
        .map(|job| job.id.clone())
        .collect();
    let mut remaining: Vec<&Job> = jobs
        .iter()
        .filter(|job| job.state == JobState::Pending)
        .collect();
    let mut plan = Vec::with_capacity(remaining.len());

    while !remaining.is_empty() {
        let next = remaining
            .iter()
            .enumerate()
            .filter(|(_, job)| {
                job.dependencies
                    .iter()
                    .all(|dependency| satisfied.contains(dependency))
            })
            .max_by(|(_, left), (_, right)| {
                left.priority
                    .cmp(&right.priority)
                    .then_with(|| right.id.cmp(&left.id))
            })
            .map(|(index, _)| index);

        let Some(index) = next else {
            let mut ids: Vec<String> = remaining.iter().map(|job| job.id.clone()).collect();
            ids.sort();
            return Err(PlanError::Cycle { remaining: ids });
        };

        let job = remaining.remove(index);
        satisfied.insert(job.id.clone());
        plan.push(job.id.clone());
    }

    Ok(plan)
}
