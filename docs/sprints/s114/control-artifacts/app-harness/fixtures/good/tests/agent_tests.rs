use release_plan::{DependencyError, Job, JobState, PlanError};

fn pending(id: &str, priority: u8, dependencies: &[&str]) -> Job {
    Job {
        id: id.to_string(),
        priority,
        state: JobState::Pending,
        dependencies: dependencies.iter().map(|value| value.to_string()).collect(),
    }
}

#[test]
fn parses_trimmed_fields_and_forward_references() {
    let jobs =
        ::release_plan::parse_manifest(" deploy | 7 | pending | build \nbuild | 3 | pending |\n")
            .unwrap();
    assert_eq!(jobs[0], pending("deploy", 7, &["build"]));
    assert_eq!(jobs[1], pending("build", 3, &[]));
}

#[test]
fn rejects_unknown_dependency() {
    let error = ::release_plan::parse_manifest("ship | 1 | pending | missing\n").unwrap_err();
    assert_eq!(
        error,
        PlanError::InvalidDependency {
            job: "ship".to_string(),
            dependency: "missing".to_string(),
            kind: DependencyError::Unknown,
        }
    );
}

#[test]
fn rejects_duplicate_and_self_dependencies() {
    let duplicate = ::release_plan::parse_manifest(
        "build | 1 | pending |\nship | 1 | pending | build, build\n",
    )
    .unwrap_err();
    assert!(matches!(
        duplicate,
        PlanError::InvalidDependency {
            kind: DependencyError::Duplicate,
            ..
        }
    ));
    let self_reference =
        ::release_plan::parse_manifest("build | 1 | pending | build\n").unwrap_err();
    assert!(matches!(
        self_reference,
        PlanError::InvalidDependency {
            kind: DependencyError::SelfReference,
            ..
        }
    ));
}

#[test]
fn completed_prerequisite_is_satisfied_and_omitted() {
    let jobs =
        ::release_plan::parse_manifest("build | 1 | done |\nship | 9 | pending | build\n")
            .unwrap();
    assert_eq!(::release_plan::build_plan(&jobs).unwrap(), ["ship"]);
}

#[test]
fn highest_ready_priority_wins() {
    let jobs = vec![pending("low", 1, &[]), pending("high", 9, &[])];
    assert_eq!(
        ::release_plan::build_plan(&jobs).unwrap(),
        ["high", "low"]
    );
}

#[test]
fn lexical_id_breaks_priority_ties() {
    let jobs = vec![pending("z", 4, &[]), pending("a", 4, &[])];
    assert_eq!(::release_plan::build_plan(&jobs).unwrap(), ["a", "z"]);
}

#[test]
fn newly_satisfied_higher_priority_job_runs_next() {
    let jobs = vec![
        pending("root", 1, &[]),
        pending("urgent", 9, &["root"]),
        pending("other", 3, &[]),
    ];
    assert_eq!(
        ::release_plan::build_plan(&jobs).unwrap(),
        ["other", "root", "urgent"]
    );
}

#[test]
fn cycle_contains_all_sorted_remaining_jobs() {
    let jobs = vec![pending("b", 1, &["a"]), pending("a", 1, &["b"])];
    assert_eq!(
        ::release_plan::build_plan(&jobs),
        Err(PlanError::Cycle {
            remaining: vec!["a".to_string(), "b".to_string()],
        })
    );
}

#[test]
fn scheduler_preserves_its_input() {
    let jobs = vec![pending("root", 2, &[]), pending("leaf", 9, &["root"])];
    let snapshot = jobs.clone();
    assert_eq!(
        ::release_plan::build_plan(&jobs).unwrap(),
        ["root", "leaf"]
    );
    assert_eq!(jobs, snapshot);
}
