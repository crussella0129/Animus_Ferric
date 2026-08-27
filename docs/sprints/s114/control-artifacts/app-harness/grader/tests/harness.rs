use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use mh_rs01_grader::{
    GradeReport, grade_candidate, sha256_bytes, sha256_file, verify_artifact_manifest,
    verify_journal_chain,
};

static FIXTURE_ID: AtomicU64 = AtomicU64::new(1);

struct FixtureRoot(PathBuf);

impl FixtureRoot {
    fn new(label: &str) -> io::Result<Self> {
        let id = FIXTURE_ID.fetch_add(1, Ordering::Relaxed);
        let target_root = if let Some(path) = option_env!("CARGO_TARGET_TMPDIR") {
            PathBuf::from(path)
        } else {
            let executable = std::env::current_exe()?;
            executable
                .parent()
                .and_then(Path::parent)
                .and_then(Path::parent)
                .ok_or_else(|| io::Error::other("test executable has no target root"))?
                .to_path_buf()
        };
        let root = target_root
            .join("self-test-workspaces")
            .join(format!("{label}-{}-{id}", std::process::id()));
        if root.exists() {
            fs::remove_dir_all(&root)?;
        }
        fs::create_dir_all(&root)?;
        Ok(Self(root))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for FixtureRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn mh_rs01_seed_baseline_and_immutability() -> io::Result<()> {
    let fixture = FixtureRoot::new("seed")?;
    let seed = fixture.path().join("seed");
    let candidate = fixture.path().join("candidate");
    write_seed(&seed)?;

    let baseline = grade_candidate(&seed, &seed)?;
    assert!(!baseline.execution_allowed());
    assert!(fails(
        &baseline,
        "path_policy",
        "missing required path: PLAN.md"
    ));
    assert!(
        seed.join("src/lib.rs")
            .read_text()?
            .contains("pub mod model;")
    );
    assert!(!seed.join("src/model.rs").exists());

    write_known_good(&candidate, &seed)?;
    let passing = grade_candidate(&candidate, &seed)?;
    assert!(passing.execution_allowed(), "{}", passing.to_jsonl());

    fs::write(candidate.join("README.md"), "changed by candidate\n")?;
    let changed = grade_candidate(&candidate, &seed)?;
    assert!(fails(
        &changed,
        "seed_immutability",
        "immutable mismatch: README.md"
    ));
    Ok(())
}

#[test]
fn bubblewrap_execution_boundary_canaries() -> io::Result<()> {
    let fixture = FixtureRoot::new("bubblewrap")?;
    let source = fixture.path().join("source");
    let target = fixture.path().join("target-output");
    let temporary = fixture.path().join("temporary");
    fs::create_dir_all(&source)?;
    fs::create_dir_all(&target)?;
    fs::create_dir_all(&temporary)?;
    fs::create_dir_all(temporary.join("cargo-home"))?;
    fs::write(source.join("sentinel.txt"), "immutable\n")?;

    let source = linux_path(&source)?;
    let target = linux_path(&target)?;
    let temporary = linux_path(&temporary)?;
    let common = bubblewrap_prefix(&source, &target, &temporary);
    let canaries = format!(
        "set -eu; \
         {common} /usr/bin/bash -eu -c {}",
        shell_quote(
            "test ! -w /workspace/sentinel.txt; \
             if printf mutation > /workspace/sentinel.txt 2>/dev/null; then exit 31; fi; \
             printf target-ok > /target/write-ok; \
             printf temp-ok > /tmp/write-ok; \
             for blocked in /.root-write /etc/write-canary /opt/write-canary \
                 /homeless/write-canary /cargo-home; do \
                 if /usr/bin/bash -c ': > \"$1\"' -- \"$blocked\" 2>/dev/null; then \
                     exit 34; \
                 fi; \
             done; \
             test ! -e /mnt/c/Users && test ! -e /root && test ! -e /home; \
             if /usr/bin/timeout 2 /usr/bin/bash -c 'printf x > /dev/tcp/1.1.1.1/53' \
                2>/dev/null; then exit 32; fi; \
             line=0; while IFS=: read -r interface _; do \
                 line=$((line + 1)); if [ \"$line\" -le 2 ]; then continue; fi; \
                 interface=${interface// /}; \
                 if [ -n \"$interface\" ] && [ \"$interface\" != lo ]; then exit 33; fi; \
             done < /proc/net/dev"
        )
    );
    let output = linux_shell(&canaries)?;
    assert_success("Bubblewrap boundary canaries", &output);
    assert_eq!(
        fs::read_to_string(fixture.path().join("source/sentinel.txt"))?,
        "immutable\n"
    );
    assert_eq!(
        fs::read_to_string(fixture.path().join("target-output/write-ok"))?,
        "target-ok"
    );
    assert_eq!(
        fs::read_to_string(fixture.path().join("temporary/write-ok"))?,
        "temp-ok"
    );

    let overrun = format!(
        "set +e; /usr/bin/timeout --signal=KILL 1 \
         {common} /usr/bin/sleep 30; code=$?; \
         test \"$code\" -eq 124 -o \"$code\" -eq 137"
    );
    let output = linux_shell(&overrun)?;
    assert_success("Bubblewrap timeout/resource cap", &output);
    Ok(())
}

#[test]
fn grader_known_good_and_violation_matrix() -> io::Result<()> {
    let fixture = FixtureRoot::new("matrix")?;
    let seed = fixture.path().join("seed");
    write_seed(&seed)?;

    let good = fixture.path().join("good");
    write_known_good(&good, &seed)?;
    let report = grade_candidate(&good, &seed)?;
    assert!(report.execution_allowed(), "{}", report.to_jsonl());
    assert_eq!(report.to_jsonl(), grade_candidate(&good, &seed)?.to_jsonl());
    let cli_results = fixture.path().join("good-results.jsonl");
    let cli = Command::new(env!("CARGO_BIN_EXE_mh-rs01-grader"))
        .arg("--candidate")
        .arg(&good)
        .arg("--seed")
        .arg(&seed)
        .arg("--results")
        .arg(&cli_results)
        .output()?;
    assert_success("static grader CLI", &cli);
    assert_eq!(cli.stdout, fs::read(&cli_results)?);
    assert_eq!(String::from_utf8_lossy(&cli.stdout), report.to_jsonl());

    let immutable = fixture.path().join("immutable");
    write_known_good(&immutable, &seed)?;
    fs::write(immutable.join("tests/contract.rs"), "// changed\n")?;
    assert_dimension(&immutable, &seed, "seed_immutability")?;

    let dependency = fixture.path().join("dependency");
    write_known_good(&dependency, &seed)?;
    fs::write(
        dependency.join("Cargo.toml"),
        "[package]\nname='release_plan'\nversion='0.1.0'\nedition='2024'\n\
         [dependencies]\nserde='1'\n",
    )?;
    assert_dimension(&dependency, &seed, "dependency_policy")?;

    let alias_process = fixture.path().join("alias-process");
    write_known_good(&alias_process, &seed)?;
    fs::write(
        alias_process.join("src/main.rs"),
        "use std::process::Command as C; fn main() { let _ = C::new(\"sh\"); }\n",
    )?;
    assert_dimension(&alias_process, &seed, "source_safety")?;

    let alias_network = fixture.path().join("alias-network");
    write_known_good(&alias_network, &seed)?;
    fs::write(
        alias_network.join("src/main.rs"),
        "use std::net::TcpStream as S; fn main() { let _ = S::connect(\"127.0.0.1:1\"); }\n",
    )?;
    assert_dimension(&alias_network, &seed, "source_safety")?;

    let path_attribute = fixture.path().join("path-attribute");
    write_known_good(&path_attribute, &seed)?;
    fs::write(
        path_attribute.join("src/main.rs"),
        "#[path = \"/hidden/escape.rs\"] mod escape; fn main() {}\n",
    )?;
    assert_dimension(&path_attribute, &seed, "source_safety")?;

    let alias_include = fixture.path().join("alias-include");
    write_known_good(&alias_include, &seed)?;
    fs::write(
        alias_include.join("src/main.rs"),
        "use std::include as graft; fn main() { graft!(\"/hidden/escape.rs\"); }\n",
    )?;
    assert_dimension(&alias_include, &seed, "source_safety")?;

    for (label, source, detail) in [
        (
            "alias-asm",
            "use core::arch::asm as emit; pub fn marker() {}\n",
            "inline assembly macro identifier: src/scheduler.rs",
        ),
        (
            "global-asm",
            "core::arch::global_asm!(\"\"); pub fn marker() {}\n",
            "global assembly macro identifier: src/scheduler.rs",
        ),
        (
            "alias-naked-asm",
            "use core::arch::naked_asm as emit; pub fn marker() {}\n",
            "naked assembly macro identifier: src/scheduler.rs",
        ),
    ] {
        let assembly = fixture.path().join(label);
        write_known_good(&assembly, &seed)?;
        fs::write(assembly.join("src/scheduler.rs"), source)?;
        let report = grade_candidate(&assembly, &seed)?;
        assert!(fails(&report, "source_safety", detail));
    }

    let library_fs = fixture.path().join("library-fs");
    write_known_good(&library_fs, &seed)?;
    fs::write(
        library_fs.join("src/scheduler.rs"),
        "pub fn inspect_executable() { let _ = std::fs::read(\"/proc/self/exe\"); }\n",
    )?;
    assert_dimension(&library_fs, &seed, "source_safety")?;

    let test_thread = fixture.path().join("test-thread");
    write_known_good(&test_thread, &seed)?;
    let mut thread_tests = fs::read_to_string(test_thread.join("tests/agent_tests.rs"))?;
    thread_tests.push_str(
        "\n#[test]\nfn inspects_test_thread() { \
         let _ = std::thread::current().name(); assert!(true); \
         }\n",
    );
    fs::write(test_thread.join("tests/agent_tests.rs"), thread_tests)?;
    assert_dimension(&test_thread, &seed, "source_safety")?;

    let library_path = fixture.path().join("library-path");
    write_known_good(&library_path, &seed)?;
    fs::write(
        library_path.join("src/scheduler.rs"),
        "use std::path::Path as CandidatePath; \
         pub fn inspect_executable() { \
             let _ = CandidatePath::new(\"/proc/self/exe\").read_link(); \
         }\n",
    )?;
    assert_dimension(&library_path, &seed, "source_safety")?;

    let caller_location = fixture.path().join("caller-location");
    write_known_good(&caller_location, &seed)?;
    fs::write(
        caller_location.join("src/scheduler.rs"),
        "#[track_caller] pub fn hidden_caller() -> &'static str { \
             std::panic::Location::caller().file() \
         }\n",
    )?;
    assert_dimension(&caller_location, &seed, "source_safety")?;

    let safe_substrings = fixture.path().join("safe-source-substrings");
    write_known_good(&safe_substrings, &seed)?;
    fs::write(
        safe_substrings.join("src/scheduler.rs"),
        "pub fn pathfinding( \
             metadata_cache: &str, caller_label: &str, assembly_state: bool, \
         ) -> bool { \
             !metadata_cache.is_empty() && !caller_label.is_empty() && assembly_state \
         }\n",
    )?;
    let safe_substrings_report = grade_candidate(&safe_substrings, &seed)?;
    assert!(
        safe_substrings_report.execution_allowed(),
        "{}",
        safe_substrings_report.to_jsonl()
    );

    let unsafe_source = fixture.path().join("unsafe");
    write_known_good(&unsafe_source, &seed)?;
    fs::write(
        unsafe_source.join("src/model.rs"),
        "pub unsafe fn bypass() {}\n",
    )?;
    assert_dimension(&unsafe_source, &seed, "source_safety")?;

    let shadowed_assert = fixture.path().join("shadowed-assert");
    write_known_good(&shadowed_assert, &seed)?;
    let assert_tests = fs::read_to_string(shadowed_assert.join("tests/agent_tests.rs"))?;
    fs::write(
        shadowed_assert.join("tests/agent_tests.rs"),
        format!("macro_rules! assert {{ ($($tokens:tt)*) => {{}}; }}\n{assert_tests}"),
    )?;
    let shadowed_assert_report = grade_candidate(&shadowed_assert, &seed)?;
    assert!(fails(
        &shadowed_assert_report,
        "source_safety",
        "local macro definition: tests/agent_tests.rs"
    ));

    let extra = fixture.path().join("extra");
    write_known_good(&extra, &seed)?;
    fs::write(extra.join("notes.txt"), "not allowed\n")?;
    assert_dimension(&extra, &seed, "path_policy")?;
    let rejected_results = fixture.path().join("rejected-results.jsonl");
    let rejected = Command::new(env!("CARGO_BIN_EXE_mh-rs01-grader"))
        .arg("--candidate")
        .arg(&extra)
        .arg("--seed")
        .arg(&seed)
        .arg("--results")
        .arg(&rejected_results)
        .output()?;
    assert_eq!(rejected.status.code(), Some(2));
    assert_eq!(rejected.stdout, fs::read(&rejected_results)?);

    let entry_limit = fixture.path().join("entry-limit");
    write_known_good(&entry_limit, &seed)?;
    for index in 0..4_091 {
        fs::write(entry_limit.join(format!("bulk-{index:04}.txt")), [])?;
    }
    let entry_limit_report = grade_candidate(&entry_limit, &seed)?;
    assert!(fails(
        &entry_limit_report,
        "path_policy",
        "filesystem entry limit exceeded: more than 4096"
    ));

    let build_script = fixture.path().join("build-script");
    write_known_good(&build_script, &seed)?;
    fs::write(build_script.join("build.rs"), "fn main() {}\n")?;
    assert_dimension(&build_script, &seed, "path_policy")?;
    assert_dimension(&build_script, &seed, "source_safety")?;

    let plan = fixture.path().join("plan");
    write_known_good(&plan, &seed)?;
    fs::write(
        plan.join("PLAN.md"),
        "## Contract\n## File plan\n## Verification\n- [ ] run authorized check\n",
    )?;
    assert_dimension(&plan, &seed, "plan")?;

    let tests = fixture.path().join("tests");
    write_known_good(&tests, &seed)?;
    fs::write(
        tests.join("tests/agent_tests.rs"),
        "#[test]\nfn only_one() {}\n",
    )?;
    assert_dimension(&tests, &seed, "model_tests")?;

    let trivial_tests = fixture.path().join("trivial-tests");
    write_known_good(&trivial_tests, &seed)?;
    fs::write(
        trivial_tests.join("tests/agent_tests.rs"),
        "#[test] fn trivial_one() {}\n\
         #[test] fn trivial_two() {}\n\
         #[test] fn trivial_three() {}\n\
         #[test] fn trivial_four() {}\n\
         #[test] fn trivial_five() {}\n\
         #[test] fn trivial_six() {}\n",
    )?;
    assert_dimension(&trivial_tests, &seed, "model_tests")?;

    let empty_topic_tests = fixture.path().join("empty-topic-tests");
    write_known_good(&empty_topic_tests, &seed)?;
    fs::write(
        empty_topic_tests.join("tests/agent_tests.rs"),
        "use release_plan::{build_plan, parse_manifest};\n\
         #[test] fn parses_manifest() {}\n\
         #[test] fn rejects_invalid_dependencies() {}\n\
         #[test] fn completed_prerequisites() {}\n\
         #[test] fn priority_ordering() {}\n\
         #[test] fn lexical_tie_breaking() {}\n\
         #[test] fn cycles_are_reported() {}\n\
         #[test] fn preserves_input_jobs() {}\n",
    )?;
    let empty_topic_report = grade_candidate(&empty_topic_tests, &seed)?;
    assert!(fails(
        &empty_topic_report,
        "model_tests",
        "model-authored test has no oracle signal: parses_manifest"
    ));

    let constant_oracle_topics = fixture.path().join("constant-oracle-topics");
    write_known_good(&constant_oracle_topics, &seed)?;
    fs::write(
        constant_oracle_topics.join("tests/agent_tests.rs"),
        "use release_plan::{build_plan, parse_manifest};\n\
         #[test] fn parses_manifest() { assert!(true); }\n\
         #[test] fn rejects_invalid_dependencies() { assert!(true); }\n\
         #[test] fn completed_prerequisites() { assert!(true); }\n\
         #[test] fn priority_ordering() { assert!(true); }\n\
         #[test] fn lexical_tie_breaking() { assert!(true); }\n\
         #[test] fn cycles_are_reported() { assert!(true); }\n\
         #[test] fn preserves_input_jobs() { assert!(true); }\n",
    )?;
    let constant_oracle_report = grade_candidate(&constant_oracle_topics, &seed)?;
    assert!(fails(
        &constant_oracle_report,
        "model_tests",
        "topical test lacks a coupled ::release_plan::parse_manifest oracle: parses_manifest"
    ));

    let function_pointer_topics = fixture.path().join("function-pointer-topics");
    write_known_good(&function_pointer_topics, &seed)?;
    fs::write(
        function_pointer_topics.join("tests/agent_tests.rs"),
        "use release_plan::{build_plan, parse_manifest};\n\
         #[test] fn parses_manifest() { let _api = parse_manifest; assert!(true); }\n\
         #[test] fn rejects_invalid_dependencies() { let _api = parse_manifest; assert!(true); }\n\
         #[test] fn completed_prerequisites() { let _api = build_plan; assert!(true); }\n\
         #[test] fn priority_ordering() { let _api = build_plan; assert!(true); }\n\
         #[test] fn lexical_tie_breaking() { let _api = build_plan; assert!(true); }\n\
         #[test] fn cycles_and_input_preservation() { let _api = build_plan; assert!(true); }\n",
    )?;
    let function_pointer_report = grade_candidate(&function_pointer_topics, &seed)?;
    assert!(fails(
        &function_pointer_report,
        "model_tests",
        "topical test lacks a coupled ::release_plan::parse_manifest oracle: parses_manifest"
    ));

    let similarly_named_calls = fixture.path().join("similarly-named-calls");
    write_known_good(&similarly_named_calls, &seed)?;
    fs::write(
        similarly_named_calls.join("tests/agent_tests.rs"),
        "use release_plan::{build_plan, parse_manifest};\n\
         fn fake_parse_manifest() {} fn fake_build_plan() {}\n\
         #[test] fn parses_manifest() { let _api = parse_manifest; fake_parse_manifest(); assert!(true); }\n\
         #[test] fn rejects_invalid_dependencies() { let _api = parse_manifest; fake_parse_manifest(); assert!(true); }\n\
         #[test] fn completed_prerequisites() { let _api = build_plan; fake_build_plan(); assert!(true); }\n\
         #[test] fn priority_ordering() { let _api = build_plan; fake_build_plan(); assert!(true); }\n\
         #[test] fn lexical_tie_breaking() { let _api = build_plan; fake_build_plan(); assert!(true); }\n\
         #[test] fn cycles_and_input_preservation() { let _api = build_plan; fake_build_plan(); assert!(true); }\n",
    )?;
    let similarly_named_report = grade_candidate(&similarly_named_calls, &seed)?;
    assert!(fails(
        &similarly_named_report,
        "model_tests",
        "topical test lacks a coupled ::release_plan::parse_manifest oracle: parses_manifest"
    ));

    let exact_name_shadow = fixture.path().join("exact-name-shadow");
    write_known_good(&exact_name_shadow, &seed)?;
    fs::write(
        exact_name_shadow.join("tests/agent_tests.rs"),
        "struct Fake; impl Fake { fn expect(self, _: &str) {} }\n\
         fn parse_manifest(_: &str) -> Fake { Fake }\n\
         fn build_plan(_: &[()]) -> Fake { Fake }\n\
         #[test] fn parses_manifest() { parse_manifest(\"\").expect(\"fake\"); }\n\
         #[test] fn rejects_invalid_dependencies() { parse_manifest(\"bad\").expect(\"fake\"); }\n\
         #[test] fn completed_prerequisites() { build_plan(&[]).expect(\"fake\"); }\n\
         #[test] fn priority_ordering() { build_plan(&[]).expect(\"fake\"); }\n\
         #[test] fn lexical_tie_breaking() { build_plan(&[]).expect(\"fake\"); }\n\
         #[test] fn cycles_and_input_preservation() { build_plan(&[]).expect(\"fake\"); }\n",
    )?;
    let exact_name_shadow_report = grade_candidate(&exact_name_shadow, &seed)?;
    assert!(fails(
        &exact_name_shadow_report,
        "model_tests",
        "topical test lacks a coupled ::release_plan::parse_manifest oracle: parses_manifest"
    ));

    let oracle_alias = fixture.path().join("oracle-alias");
    write_known_good(&oracle_alias, &seed)?;
    fs::write(
        oracle_alias.join("tests/agent_tests.rs"),
        "use std::println as assert;\n\
         #[test] fn parses_manifest() { let _ = ::release_plan::parse_manifest(\"\").unwrap(); assert!(\"noop\"); }\n\
         #[test] fn rejects_invalid_dependencies() { let _ = ::release_plan::parse_manifest(\"\").unwrap(); assert!(\"noop\"); }\n\
         #[test] fn completed_prerequisites() { let _ = ::release_plan::build_plan(&[]).unwrap(); assert!(\"noop\"); }\n\
         #[test] fn priority_ordering() { let _ = ::release_plan::build_plan(&[]).unwrap(); assert!(\"noop\"); }\n\
         #[test] fn lexical_tie_breaking() { let _ = ::release_plan::build_plan(&[]).unwrap(); assert!(\"noop\"); }\n\
         #[test] fn cycles_and_input_preservation() { let _ = ::release_plan::build_plan(&[]).unwrap(); assert!(\"noop\"); }\n",
    )?;
    let oracle_alias_report = grade_candidate(&oracle_alias, &seed)?;
    assert!(fails(
        &oracle_alias_report,
        "model_tests",
        "oracle macro alias is forbidden: tests/agent_tests.rs"
    ));

    let unrelated_fake_oracle = fixture.path().join("unrelated-fake-oracle");
    write_known_good(&unrelated_fake_oracle, &seed)?;
    fs::write(
        unrelated_fake_oracle.join("tests/agent_tests.rs"),
        "struct Fake; impl Fake { fn expect(self, _: &str) {} }\n\
         #[test] fn parses_manifest() { let _ = ::release_plan::parse_manifest(\"\"); Fake.expect(\"fake\"); }\n\
         #[test] fn rejects_invalid_dependencies() { let _ = ::release_plan::parse_manifest(\"bad\"); Fake.expect(\"fake\"); }\n\
         #[test] fn completed_prerequisites() { let _ = ::release_plan::build_plan(&[]); Fake.expect(\"fake\"); }\n\
         #[test] fn priority_ordering() { let _ = ::release_plan::build_plan(&[]); Fake.expect(\"fake\"); }\n\
         #[test] fn lexical_tie_breaking() { let _ = ::release_plan::build_plan(&[]); Fake.expect(\"fake\"); }\n\
         #[test] fn cycles_and_input_preservation() { let _ = ::release_plan::build_plan(&[]); Fake.expect(\"fake\"); }\n",
    )?;
    let unrelated_fake_report = grade_candidate(&unrelated_fake_oracle, &seed)?;
    assert!(fails(
        &unrelated_fake_report,
        "model_tests",
        "topical test lacks a coupled ::release_plan::parse_manifest oracle: parses_manifest"
    ));

    let fallback_oracle_topics = fixture.path().join("fallback-oracle-topics");
    write_known_good(&fallback_oracle_topics, &seed)?;
    fs::write(
        fallback_oracle_topics.join("tests/agent_tests.rs"),
        "#[test] fn parses_manifest() { \
             let _ = ::release_plan::parse_manifest(\"\").unwrap_or_default(); \
         }\n\
         #[test] fn rejects_invalid_dependencies() { \
             let _ = ::release_plan::parse_manifest(\"invalid\").unwrap_or_else(|_| Vec::new()); \
         }\n\
         #[test] fn completed_prerequisites() { assert!(::release_plan::build_plan(&[]).is_ok()); }\n\
         #[test] fn priority_ordering() { assert!(::release_plan::build_plan(&[]).is_ok()); }\n\
         #[test] fn lexical_tie_breaking() { assert!(::release_plan::build_plan(&[]).is_ok()); }\n\
         #[test] fn cycles_are_reported() { assert!(::release_plan::build_plan(&[]).is_ok()); }\n\
         #[test] fn preserves_input_jobs() { assert!(::release_plan::build_plan(&[]).is_ok()); }\n",
    )?;
    let fallback_oracle_report = grade_candidate(&fallback_oracle_topics, &seed)?;
    assert!(fails(
        &fallback_oracle_report,
        "model_tests",
        "model-authored test has no oracle signal: parses_manifest"
    ));

    let bounded_details = fixture.path().join("bounded-details");
    write_known_good(&bounded_details, &seed)?;
    let mut many_tests = fs::read_to_string(bounded_details.join("tests/agent_tests.rs"))?;
    for index in 0..4_096 {
        many_tests.push_str(&format!("\n#[test] fn noise_{index:04}() {{}}\n"));
    }
    fs::write(bounded_details.join("tests/agent_tests.rs"), many_tests)?;
    let bounded_report = grade_candidate(&bounded_details, &seed)?;
    let repeated_bounded_report = grade_candidate(&bounded_details, &seed)?;
    assert_eq!(
        bounded_report.to_jsonl(),
        repeated_bounded_report.to_jsonl()
    );
    let model_details = &bounded_report
        .dimensions
        .iter()
        .find(|result| result.dimension == "model_tests")
        .expect("model_tests dimension")
        .details;
    assert_eq!(
        model_details,
        &[
            "additional_violations: 4094",
            "model-authored test has no oracle signal: noise_0000",
            "model-authored test has no oracle signal: noise_0001",
        ]
    );
    assert!(bounded_report.to_jsonl().len() < 12 * 1024);

    let ordinary_cfg_ignore = fixture.path().join("ordinary-cfg-ignore");
    write_known_good(&ordinary_cfg_ignore, &seed)?;
    let mut ordinary_tests = fs::read_to_string(ordinary_cfg_ignore.join("tests/agent_tests.rs"))?;
    ordinary_tests.push_str(
        "\n#[test]\nfn ordinary_cfg_and_ignore_identifiers() { \
             let cfg = cfg!(test); let ignore = cfg; assert!(ignore); \
         }\n",
    );
    fs::write(
        ordinary_cfg_ignore.join("tests/agent_tests.rs"),
        ordinary_tests,
    )?;
    let ordinary_report = grade_candidate(&ordinary_cfg_ignore, &seed)?;
    assert!(
        ordinary_report.execution_allowed(),
        "{}",
        ordinary_report.to_jsonl()
    );

    let allowed_test_cfg = fixture.path().join("allowed-test-cfg");
    write_known_good(&allowed_test_cfg, &seed)?;
    let cfg_tests = fs::read_to_string(allowed_test_cfg.join("tests/agent_tests.rs"))?;
    fs::write(
        allowed_test_cfg.join("tests/agent_tests.rs"),
        cfg_tests.replace("#[test]", "#[cfg(test)]\n#[test]"),
    )?;
    let allowed_cfg_report = grade_candidate(&allowed_test_cfg, &seed)?;
    assert!(
        allowed_cfg_report.execution_allowed(),
        "{}",
        allowed_cfg_report.to_jsonl()
    );

    let ignored_tests = fixture.path().join("ignored-tests");
    write_known_good(&ignored_tests, &seed)?;
    let ignored_source = fs::read_to_string(ignored_tests.join("tests/agent_tests.rs"))?;
    fs::write(
        ignored_tests.join("tests/agent_tests.rs"),
        ignored_source.replacen("#[test]", "#[ignore]\n#[test]", 1),
    )?;
    assert_dimension(&ignored_tests, &seed, "model_tests")?;

    let conditionally_ignored = fixture.path().join("conditionally-ignored");
    write_known_good(&conditionally_ignored, &seed)?;
    let conditional_source =
        fs::read_to_string(conditionally_ignored.join("tests/agent_tests.rs"))?;
    fs::write(
        conditionally_ignored.join("tests/agent_tests.rs"),
        conditional_source.replacen("#[test]", "#[cfg_attr(test, ignore)]\n#[test]", 1),
    )?;
    assert_dimension(&conditionally_ignored, &seed, "model_tests")?;

    let disabled_tests = fixture.path().join("disabled-tests");
    write_known_good(&disabled_tests, &seed)?;
    let valid_tests = fs::read_to_string(disabled_tests.join("tests/agent_tests.rs"))?;
    fs::write(
        disabled_tests.join("tests/agent_tests.rs"),
        valid_tests.replace("#[test]", "#[cfg(any())]\n#[test]"),
    )?;
    assert_dimension(&disabled_tests, &seed, "model_tests")?;

    let link = fixture.path().join("symlink");
    write_known_good(&link, &seed)?;
    create_file_symlink(link.join("README.md"), link.join("escape-link"))?;
    assert_dimension(&link, &seed, "path_policy")?;

    let outside_source = fixture.path().join("outside-source");
    fs::create_dir(&outside_source)?;
    fs::write(
        outside_source.join("model.rs"),
        "compile_error!(\"trusted grader followed an ancestor symlink\");\n",
    )?;
    let ancestor_link = fixture.path().join("ancestor-symlink");
    write_known_good(&ancestor_link, &seed)?;
    fs::remove_dir_all(ancestor_link.join("src"))?;
    create_directory_symlink(outside_source, ancestor_link.join("src"))?;
    let ancestor_link_report = grade_candidate(&ancestor_link, &seed)?;
    assert!(fails(&ancestor_link_report, "path_policy", "symlink: src"));
    assert!(fails(
        &ancestor_link_report,
        "seed_immutability",
        "not evaluated because candidate inventory is unsafe"
    ));

    #[cfg(unix)]
    {
        let hardlink = fixture.path().join("hardlink");
        write_known_good(&hardlink, &seed)?;
        fs::remove_file(hardlink.join("src/model.rs"))?;
        fs::hard_link(
            hardlink.join("src/parser.rs"),
            hardlink.join("src/model.rs"),
        )?;
        let hardlink_report = grade_candidate(&hardlink, &seed)?;
        assert!(fails(
            &hardlink_report,
            "path_policy",
            "hardlink: src/model.rs"
        ));
    }

    let required_link = fixture.path().join("required-symlink");
    write_known_good(&required_link, &seed)?;
    fs::remove_file(required_link.join("src/model.rs"))?;
    create_file_symlink(
        required_link.join("README.md"),
        required_link.join("src/model.rs"),
    )?;
    let required_link_report = grade_candidate(&required_link, &seed)?;
    assert!(fails(
        &required_link_report,
        "path_policy",
        "symlink: src/model.rs"
    ));

    let required_directory = fixture.path().join("required-directory");
    write_known_good(&required_directory, &seed)?;
    fs::remove_file(required_directory.join("PLAN.md"))?;
    fs::create_dir(required_directory.join("PLAN.md"))?;
    let required_directory_report = grade_candidate(&required_directory, &seed)?;
    assert!(fails(
        &required_directory_report,
        "path_policy",
        "required path is not a regular file: PLAN.md"
    ));
    assert!(fails(
        &required_directory_report,
        "plan",
        "PLAN.md is not a regular file"
    ));
    let required_directory_cli = Command::new(env!("CARGO_BIN_EXE_mh-rs01-grader"))
        .arg("--candidate")
        .arg(&required_directory)
        .arg("--seed")
        .arg(&seed)
        .output()?;
    assert_eq!(required_directory_cli.status.code(), Some(2));

    let library_exit = fixture.path().join("library-exit");
    write_known_good(&library_exit, &seed)?;
    fs::write(
        library_exit.join("src/scheduler.rs"),
        "pub fn bypass() { std::process::exit(0); }\n",
    )?;
    assert_dimension(&library_exit, &seed, "source_safety")?;

    let aliased_library_exit = fixture.path().join("aliased-library-exit");
    write_known_good(&aliased_library_exit, &seed)?;
    fs::write(
        aliased_library_exit.join("src/scheduler.rs"),
        "use std::process as p; pub fn bypass() { p::exit(0); }\n",
    )?;
    assert_dimension(&aliased_library_exit, &seed, "source_safety")?;

    let harmless_exit_name = fixture.path().join("harmless-exit-name");
    write_known_good(&harmless_exit_name, &seed)?;
    let mut harmless_tests = fs::read_to_string(harmless_exit_name.join("tests/agent_tests.rs"))?;
    harmless_tests
        .push_str("\n#[test]\nfn invalid_input_exits_nonzero() { assert_eq!(2 + 2, 4); }\n");
    fs::write(
        harmless_exit_name.join("tests/agent_tests.rs"),
        harmless_tests,
    )?;
    let harmless_report = grade_candidate(&harmless_exit_name, &seed)?;
    assert!(
        harmless_report.execution_allowed(),
        "{}",
        harmless_report.to_jsonl()
    );

    Ok(())
}

#[test]
fn journal_chain_and_artifact_hashes() -> io::Result<()> {
    let fixture = FixtureRoot::new("journal")?;
    let root = fixture.path().join("artifacts");
    fs::create_dir_all(root.join("nested"))?;
    fs::write(root.join("a.txt"), "alpha\n")?;
    fs::write(root.join("nested/b.txt"), "beta\n")?;
    let a_hash = sha256_file(&root.join("a.txt"))?;
    let b_hash = sha256_file(&root.join("nested/b.txt"))?;
    let manifest = format!("{a_hash}  a.txt\n{b_hash}  nested/b.txt\n");
    assert!(verify_artifact_manifest(&root, &manifest)?.is_empty());
    assert_eq!(
        verify_artifact_manifest(&root, "")?,
        vec!["manifest contains no artifacts"]
    );

    let invalid_manifest = manifest.replacen(&a_hash, &"0".repeat(64), 1);
    assert_eq!(
        verify_artifact_manifest(&root, &invalid_manifest)?,
        vec!["artifact hash mismatch: a.txt"]
    );
    create_file_symlink(root.join("a.txt"), root.join("linked.txt"))?;
    let linked_manifest = format!("{a_hash}  linked.txt\n");
    let linked_failures = verify_artifact_manifest(&root, &linked_manifest)?;
    assert_eq!(linked_failures.len(), 1);
    assert!(linked_failures[0].starts_with("artifact unavailable: linked.txt:"));

    let header = "schema\tsequence\tprevious_sha256\tstage_b64\tcwd_b64\targv_b64\t\
        exit_code\tstdout_path_b64\tstdout_sha256\tstderr_path_b64\tstderr_sha256\tentry_sha256";
    let zero_hash = "0".repeat(64);
    let first_prefix = format!(
        "s114-command-journal-v1\t1\t{zero_hash}\tY3JlYXRlZA==\tL3dvcmtzcGFjZQ==\tY2FyZ28=\t0\t\
         L2xvZ3Mvc3Rkb3V0\t{a_hash}\tL2xvZ3Mvc3RkZXJy\t{b_hash}"
    );
    let first_hash = sha256_bytes(first_prefix.as_bytes());
    let second_prefix = format!(
        "s114-command-journal-v1\t2\t{first_hash}\tdmVyaWZpZWQ=\tL3dvcmtzcGFjZQ==\t\
         Y2FyZ28=,dGVzdA==\t2\tL2xvZ3Mvc3Rkb3V0\t{b_hash}\tL2xvZ3Mvc3RkZXJy\t{a_hash}"
    );
    let second_hash = sha256_bytes(second_prefix.as_bytes());
    let journal =
        format!("{header}\n{first_prefix}\t{first_hash}\n{second_prefix}\t{second_hash}\n");
    assert!(verify_journal_chain(&journal).is_empty());

    let tampered = journal.replacen("\tdmVyaWZpZWQ=\t", "\tcmVwYWlyZWQ=\t", 1);
    assert_eq!(
        verify_journal_chain(&tampered),
        vec!["journal line 3 has wrong entry hash"]
    );
    assert_eq!(
        verify_journal_chain(header),
        vec!["journal contains no command records"]
    );
    Ok(())
}

fn write_seed(root: &Path) -> io::Result<()> {
    fs::create_dir_all(root.join("src"))?;
    fs::create_dir_all(root.join("tests"))?;
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"release_plan\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\
         \n[dependencies]\n",
    )?;
    fs::write(
        root.join("Cargo.lock"),
        "# This file is automatically @generated by Cargo.\nversion = 4\n\n\
         [[package]]\nname = \"release_plan\"\nversion = \"0.1.0\"\n",
    )?;
    fs::write(root.join("README.md"), "# release_plan\n")?;
    fs::write(
        root.join("src/lib.rs"),
        "pub mod model;\npub mod parser;\npub mod scheduler;\n",
    )?;
    fs::write(
        root.join("tests/contract.rs"),
        "#[test]\nfn seed_contract() {}\n",
    )?;
    Ok(())
}

fn write_known_good(candidate: &Path, seed: &Path) -> io::Result<()> {
    fs::create_dir_all(candidate.join("src"))?;
    fs::create_dir_all(candidate.join("tests"))?;
    for relative in [
        "Cargo.toml",
        "Cargo.lock",
        "README.md",
        "src/lib.rs",
        "tests/contract.rs",
    ] {
        fs::copy(seed.join(relative), candidate.join(relative))?;
    }
    fs::write(
        candidate.join("PLAN.md"),
        "## Contract\n\n## File plan\n\n\
         - [x] implement model.rs\n\
         - [x] implement parser.rs\n\
         - [x] implement scheduler.rs\n\
         - [x] implement main.rs\n\
         - [x] add agent_tests.rs\n\n\
         ## Verification\n\n- [x] run authorized check\n",
    )?;
    for relative in ["src/model.rs", "src/parser.rs", "src/scheduler.rs"] {
        fs::write(candidate.join(relative), "pub fn marker() {}\n")?;
    }
    fs::write(candidate.join("src/main.rs"), "fn main() {}\n")?;
    fs::write(
        candidate.join("tests/agent_tests.rs"),
        "#[test]\nfn parsing() { assert!(::release_plan::parse_manifest(\"\").is_ok()); }\n\
         #[test]\nfn invalid_dependencies() { \
             assert!(::release_plan::parse_manifest(\"a | 1 | pending | missing\\n\").is_err()); \
         }\n\
         #[test]\nfn completed_prerequisites() { \
             let jobs = ::release_plan::parse_manifest(\"done | 1 | done |\\nnext | 2 | pending | done\\n\").unwrap(); \
             assert!(::release_plan::build_plan(&jobs).is_ok()); \
         }\n\
         #[test]\nfn priority_ordering() { \
             let jobs = ::release_plan::parse_manifest(\"low | 1 | pending |\\nhigh | 9 | pending |\\n\").unwrap(); \
             assert_eq!(::release_plan::build_plan(&jobs).unwrap(), [\"high\", \"low\"]); \
         }\n\
         #[test]\nfn lexical_ties() { \
             let jobs = ::release_plan::parse_manifest(\"z | 4 | pending |\\na | 4 | pending |\\n\").unwrap(); \
             assert_eq!(::release_plan::build_plan(&jobs).unwrap(), [\"a\", \"z\"]); \
         }\n\
         #[test]\nfn cycles_and_input_preservation() { \
             let jobs = ::release_plan::parse_manifest(\"a | 1 | pending | b\\nb | 1 | pending | a\\n\").unwrap(); \
             let before = jobs.clone(); \
             assert!(::release_plan::build_plan(&jobs).is_err()); \
             assert_eq!(jobs, before); \
         }\n",
    )?;
    Ok(())
}

fn assert_dimension(candidate: &Path, seed: &Path, dimension: &str) -> io::Result<()> {
    let report = grade_candidate(candidate, seed)?;
    assert!(
        report
            .dimensions
            .iter()
            .any(|result| result.dimension == dimension && !result.passed),
        "expected {dimension} failure:\n{}",
        report.to_jsonl()
    );
    Ok(())
}

fn fails(report: &GradeReport, dimension: &str, detail: &str) -> bool {
    report.dimensions.iter().any(|result| {
        result.dimension == dimension
            && !result.passed
            && result.details.iter().any(|candidate| candidate == detail)
    })
}

#[cfg(unix)]
fn create_file_symlink(original: PathBuf, link: PathBuf) -> io::Result<()> {
    std::os::unix::fs::symlink(original, link)
}

#[cfg(windows)]
fn create_file_symlink(original: PathBuf, link: PathBuf) -> io::Result<()> {
    std::os::windows::fs::symlink_file(original, link)
}

#[cfg(unix)]
fn create_directory_symlink(original: PathBuf, link: PathBuf) -> io::Result<()> {
    std::os::unix::fs::symlink(original, link)
}

#[cfg(windows)]
fn create_directory_symlink(original: PathBuf, link: PathBuf) -> io::Result<()> {
    std::os::windows::fs::symlink_dir(original, link)
}

fn linux_path(path: &Path) -> io::Result<String> {
    #[cfg(windows)]
    {
        let output = Command::new("wsl.exe")
            .arg("--exec")
            .arg("wslpath")
            .arg("-a")
            .arg(path)
            .output()?;
        if !output.status.success() {
            return Err(command_error("wslpath", &output));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }
    #[cfg(not(windows))]
    {
        Ok(path.to_string_lossy().into_owned())
    }
}

fn linux_shell(script: &str) -> io::Result<Output> {
    #[cfg(windows)]
    {
        Command::new("wsl.exe")
            .args(["--exec", "bash", "-lc", script])
            .output()
    }
    #[cfg(not(windows))]
    {
        Command::new("bash").args(["-lc", script]).output()
    }
}

fn bubblewrap_prefix(source: &str, target: &str, temporary: &str) -> String {
    format!(
        "/usr/bin/prlimit --as=536870912 --nproc=64 --cpu=4 -- \
         /usr/bin/bwrap --die-with-parent --new-session --unshare-all --clearenv \
         --tmpfs / --proc /proc --dev /dev \
         --dir /etc --dir /opt --dir /workspace --dir /target --dir /tmp --dir /homeless \
         --ro-bind /usr /usr --ro-bind /lib /lib --ro-bind /lib64 /lib64 \
         --ro-bind {} /workspace --bind {} /target --bind {} /tmp \
         --remount-ro / \
         --setenv HOME /homeless --setenv CARGO_HOME /tmp/cargo-home \
         --setenv TMPDIR /tmp --setenv PATH /usr/bin:/bin \
         --chdir /workspace --cap-drop ALL",
        shell_quote(source),
        shell_quote(target),
        shell_quote(temporary)
    )
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn assert_success(label: &str, output: &Output) {
    assert!(
        output.status.success(),
        "{label} failed with {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(windows)]
fn command_error(label: &str, output: &Output) -> io::Error {
    io::Error::other(format!(
        "{label} failed with {:?}: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    ))
}

trait ReadText {
    fn read_text(&self) -> io::Result<String>;
}

impl ReadText for PathBuf {
    fn read_text(&self) -> io::Result<String> {
        fs::read_to_string(self)
    }
}
