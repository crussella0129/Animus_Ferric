//! External, operator-owned semantic checks for the MH-RS01 library.

#[cfg(test)]
mod tests {
    use release_plan::{PlanError, build_plan, parse_manifest};

    #[test]
    fn parsing_trims_fields_comments_and_dependencies() {
        let jobs = parse_manifest(
            "\n # ignored\n deploy | 7 | pending | build, test \n\
             build | 4 | done |\n\
             test | 5 | pending | build\n",
        )
        .expect("valid manifest");
        assert_eq!(jobs.len(), 3);
        assert_eq!(jobs[0].id, "deploy");
        assert_eq!(jobs[0].priority, 7);
        assert_eq!(jobs[0].dependencies, ["build", "test"]);
    }

    #[test]
    fn priority_then_lexical_order_is_deterministic() {
        let jobs = parse_manifest(
            "zulu | 9 | pending |\n\
             alpha | 9 | pending |\n\
             middle | 4 | pending |\n",
        )
        .expect("valid manifest");
        assert_eq!(
            build_plan(&jobs).expect("acyclic plan"),
            ["alpha", "zulu", "middle"]
        );
    }

    #[test]
    fn completed_jobs_satisfy_dependencies_and_are_omitted() {
        let jobs = parse_manifest(
            "compiled | 9 | done |\n\
             package | 4 | pending | compiled\n\
             publish | 8 | pending | package\n",
        )
        .expect("valid manifest");
        assert_eq!(
            build_plan(&jobs).expect("completed prerequisite is satisfied"),
            ["package", "publish"]
        );
    }

    #[test]
    fn parser_rejects_every_dependency_integrity_violation() {
        for manifest in [
            "a | 1 | pending | missing\n",
            "a | 1 | pending | a\n",
            "a | 1 | pending | b,b\nb | 1 | done |\n",
            "a | 1 | pending | ,b\nb | 1 | done |\n",
            "a | 1 | pending | b,,b\nb | 1 | done |\n",
            "a | 1 | pending |\na | 2 | done |\n",
            " | 1 | pending |\n",
            "a | -1 | pending |\n",
            "a | 10 | pending |\n",
            "a | high | pending |\n",
            "a | 1 | Pending |\n",
            "a | 1 | waiting |\n",
            "a | 1 | pending\n",
            "a | 1 | pending | | extra\n",
        ] {
            assert!(parse_manifest(manifest).is_err(), "accepted: {manifest:?}");
        }
    }

    #[test]
    fn cycle_reports_all_remaining_ids_sorted() {
        let jobs = parse_manifest(
            "z | 9 | pending | y\n\
             y | 8 | pending | z\n\
             blocked | 7 | pending | y\n\
             ready | 1 | pending |\n",
        )
        .expect("valid manifest");
        let error = build_plan(&jobs).expect_err("cycle must fail");
        match error {
            PlanError::Cycle { remaining } => assert_eq!(remaining, ["blocked", "y", "z"]),
            other => panic!("expected cycle, got {other:?}"),
        }
    }

    #[test]
    fn planning_does_not_mutate_input_jobs() {
        let jobs = parse_manifest(
            "first | 2 | pending |\n\
             second | 3 | pending | first\n",
        )
        .expect("valid manifest");
        let before: Vec<_> = jobs
            .iter()
            .map(|job| {
                (
                    job.id.clone(),
                    job.priority,
                    format!("{:?}", job.state),
                    job.dependencies.clone(),
                )
            })
            .collect();
        assert_eq!(build_plan(&jobs).expect("valid plan"), ["first", "second"]);
        let after: Vec<_> = jobs
            .iter()
            .map(|job| {
                (
                    job.id.clone(),
                    job.priority,
                    format!("{:?}", job.state),
                    job.dependencies.clone(),
                )
            })
            .collect();
        assert_eq!(before, after);
    }
}
