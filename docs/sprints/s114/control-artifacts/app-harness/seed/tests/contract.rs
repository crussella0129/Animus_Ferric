use release_plan::{DependencyError, Job, JobState, PlanError, build_plan, parse_manifest};

fn assert_debug_clone_partial_eq_eq<T: std::fmt::Debug + Clone + PartialEq + Eq>() {}

fn assert_copy<T: Copy>() {}

fn job(id: &str, priority: u8, state: JobState, dependencies: &[&str]) -> Job {
    Job {
        id: id.to_string(),
        priority,
        state,
        dependencies: dependencies.iter().map(|value| value.to_string()).collect(),
    }
}

#[test]
fn public_types_have_the_frozen_shape() {
    assert_debug_clone_partial_eq_eq::<Job>();
    assert_debug_clone_partial_eq_eq::<JobState>();
    assert_debug_clone_partial_eq_eq::<DependencyError>();
    assert_debug_clone_partial_eq_eq::<PlanError>();
    assert_copy::<JobState>();
    assert_copy::<DependencyError>();

    let candidate = job("package", 7, JobState::Pending, &["build"]);
    assert_eq!(candidate.id, "package");
    assert_eq!(candidate.priority, 7);
    assert_eq!(candidate.state, JobState::Pending);
    assert_eq!(candidate.dependencies, ["build"]);

    let dependency_error = PlanError::InvalidDependency {
        job: "package".to_string(),
        dependency: "build".to_string(),
        kind: DependencyError::Unknown,
    };
    assert!(!dependency_error.to_string().is_empty());
    let _: &dyn std::error::Error = &dependency_error;

    let state = match JobState::Pending {
        JobState::Pending => 0,
        JobState::Done => 1,
    };
    assert_eq!(state, 0);

    let dependency_kind = match DependencyError::Unknown {
        DependencyError::Empty => 0,
        DependencyError::Duplicate => 1,
        DependencyError::SelfReference => 2,
        DependencyError::Unknown => 3,
    };
    assert_eq!(dependency_kind, 3);

    let error_shape = match dependency_error {
        PlanError::InvalidLine { line, reason } => (line, reason),
        PlanError::DuplicateId { id } => (0, id),
        PlanError::InvalidDependency {
            job,
            dependency,
            kind,
        } => (
            usize::from(kind == DependencyError::Unknown),
            format!("{job}:{dependency}"),
        ),
        PlanError::Cycle { remaining } => (remaining.len(), remaining.join(":")),
    };
    assert_eq!(error_shape, (1, "package:build".to_string()));
}

#[test]
fn parser_ignores_comments_and_preserves_manifest_order() {
    let manifest = "\n # release jobs\nship | 9 | pending | test\ntest | 4 | done |\n";
    let jobs = parse_manifest(manifest).expect("valid manifest");

    assert_eq!(jobs.len(), 2);
    assert_eq!(jobs[0], job("ship", 9, JobState::Pending, &["test"]));
    assert_eq!(jobs[1], job("test", 4, JobState::Done, &[]));
}

#[test]
fn a_wholly_empty_dependency_field_is_valid() {
    let jobs = parse_manifest("root | 0 | pending |   \n").expect("empty dependency list");
    assert!(jobs[0].dependencies.is_empty());
}

#[test]
fn comma_created_empty_dependencies_are_rejected() {
    for dependencies in [",root", "root,", "root,,root"] {
        let manifest = format!("root | 0 | done |\nship | 1 | pending | {dependencies}\n");
        assert_eq!(
            parse_manifest(&manifest),
            Err(PlanError::InvalidDependency {
                job: "ship".to_string(),
                dependency: String::new(),
                kind: DependencyError::Empty,
            }),
            "accepted comma-created empty item in {dependencies:?}"
        );
    }
}

#[test]
fn invalid_lines_report_the_physical_input_line() {
    let error = parse_manifest("\n  # retained physical line\nbroken | 1 | pending\n")
        .expect_err("three fields must be invalid");
    match error {
        PlanError::InvalidLine { line: 3, reason } => assert!(!reason.is_empty()),
        other => panic!("expected InvalidLine at physical line 3, got {other:?}"),
    }
}

#[test]
fn duplicate_ids_have_the_exact_error_classification() {
    assert_eq!(
        parse_manifest("same | 1 | pending |\nsame | 2 | done |\n"),
        Err(PlanError::DuplicateId {
            id: "same".to_string(),
        })
    );
}

#[test]
fn duplicate_dependencies_have_the_exact_error_classification() {
    assert_eq!(
        parse_manifest("root | 0 | done |\nship | 1 | pending | root, root\n"),
        Err(PlanError::InvalidDependency {
            job: "ship".to_string(),
            dependency: "root".to_string(),
            kind: DependencyError::Duplicate,
        })
    );
}

#[test]
fn self_dependencies_have_the_exact_error_classification() {
    assert_eq!(
        parse_manifest("ship | 1 | pending | ship\n"),
        Err(PlanError::InvalidDependency {
            job: "ship".to_string(),
            dependency: "ship".to_string(),
            kind: DependencyError::SelfReference,
        })
    );
}

#[test]
fn unknown_dependencies_have_the_exact_error_classification() {
    assert_eq!(
        parse_manifest("ship | 1 | pending | absent\n"),
        Err(PlanError::InvalidDependency {
            job: "ship".to_string(),
            dependency: "absent".to_string(),
            kind: DependencyError::Unknown,
        })
    );
}

#[test]
fn completed_jobs_satisfy_dependencies_and_are_omitted() {
    let jobs = parse_manifest(
        "compile | 8 | done |\npackage | 5 | pending | compile\npublish | 9 | pending | package\n",
    )
    .expect("valid manifest");

    assert_eq!(build_plan(&jobs).unwrap(), ["package", "publish"]);
}

#[test]
fn priority_then_lexical_id_determine_each_ready_choice() {
    let jobs = vec![
        job("zeta", 9, JobState::Pending, &[]),
        job("alpha", 9, JobState::Pending, &[]),
        job("middle", 5, JobState::Pending, &[]),
    ];
    assert_eq!(build_plan(&jobs).unwrap(), ["alpha", "zeta", "middle"]);
}

#[test]
fn cycles_report_sorted_remaining_ids() {
    let jobs = vec![
        job("zeta", 1, JobState::Pending, &["alpha"]),
        job("alpha", 1, JobState::Pending, &["zeta"]),
    ];
    assert_eq!(
        build_plan(&jobs),
        Err(PlanError::Cycle {
            remaining: vec!["alpha".to_string(), "zeta".to_string()],
        })
    );
}

#[test]
fn planning_does_not_mutate_jobs() {
    let jobs = vec![
        job("prepare", 1, JobState::Pending, &[]),
        job("ship", 2, JobState::Pending, &["prepare"]),
    ];
    let before = jobs.clone();
    let _ = build_plan(&jobs).unwrap();
    assert_eq!(jobs, before);
}
