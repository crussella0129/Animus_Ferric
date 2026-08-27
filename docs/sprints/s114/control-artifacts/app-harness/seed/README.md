# release_plan

`release_plan` parses a release manifest and prints a deterministic execution
order. The crate must remain dependency-free and use Rust edition 2024.

## Input contract

The input format is `id | priority | state | dependencies`.

- Ignore blank lines and lines whose trimmed form starts with `#`.
- Every other physical line has exactly four pipe-delimited fields. Line
  numbers in errors are one-based physical input line numbers.
- Trim every field. Job IDs are non-empty and unique. A priority is an integer
  from 0 through 9. State is exactly `pending` or `done`.
- A completely empty trimmed dependencies field means no dependencies and is
  valid. Otherwise, split it on commas and trim each item. Empty items caused
  by leading, trailing, or consecutive commas are invalid. Dependency IDs may
  be forward references, but every dependency must be known, unique within its
  job, and different from the containing job's ID.
- Preserve manifest order in the returned `Vec<Job>` and preserve listed
  dependency order within each job.

## Public library API

`src/model.rs` must define these public types with exactly these fields and
variants. Each type derives `Debug`, `Clone`, `PartialEq`, and `Eq`; the two
fieldless enums also derive `Copy`. `PlanError` implements
`std::fmt::Display` and `std::error::Error`.

```rust
pub struct Job {
    pub id: String,
    pub priority: u8,
    pub state: JobState,
    pub dependencies: Vec<String>,
}

pub enum JobState {
    Pending,
    Done,
}

pub enum DependencyError {
    Empty,
    Duplicate,
    SelfReference,
    Unknown,
}

pub enum PlanError {
    InvalidLine { line: usize, reason: String },
    DuplicateId { id: String },
    InvalidDependency {
        job: String,
        dependency: String,
        kind: DependencyError,
    },
    Cycle { remaining: Vec<String> },
}
```

`src/lib.rs` exposes the types above and these functions:

```rust
pub fn parse_manifest(input: &str) -> Result<Vec<Job>, PlanError>;
pub fn build_plan(jobs: &[Job]) -> Result<Vec<String>, PlanError>;
```

`parse_manifest` enforces the complete input contract. `build_plan` accepts
parsed jobs without mutating them. Completed jobs satisfy dependencies and are
omitted from output. Repeatedly choose a ready pending job by highest numeric
priority, breaking ties by lexicographically smallest ID. A cycle returns
`PlanError::Cycle` whose `remaining` IDs are sorted in ascending lexical order.

## CLI contract

The `release_plan` binary accepts exactly one manifest path. On success, print
the planned IDs one per line. A non-empty result ends with a newline; an empty
result prints no bytes. On an argument, I/O, or manifest/plan error, print a
diagnostic beginning with `error:` to stderr and exit nonzero.

## Required work and safety boundary

Create only `PLAN.md`, `src/model.rs`, `src/parser.rs`, `src/scheduler.rs`,
`src/main.rs`, and `tests/agent_tests.rs`. Do not change this README,
`Cargo.toml`, `Cargo.lock`, `src/lib.rs`, or `tests/contract.rs`. Do not add a
dependency, build script, Cargo configuration, unsafe code, shell execution,
or network access.

### Mechanical model-test gate

`tests/agent_tests.rs` must contain at least six distinct `#[test] fn` names.
Taken together, those names must identify all seven required topics using the
following ASCII-case-insensitive stems (one name may cover multiple topics):

- parsing: `pars`, or `manifest` together with `valid` or `accept`;
- invalid dependencies: `depend` together with `invalid`, `reject`, `unknown`,
  `duplicate`, `self`, `empty`, or `error`; alternatively, `unknown`,
  `duplicate`, `self`, or `empty` together with `reject` or `invalid`;
- completed prerequisites: `completed`, or `done` together with `prereq`,
  `depend`, `job`, `omit`, `satisf`, or `unlock`;
- priority ordering: `priority`, or both `highest` and `ready`;
- lexical tie-breaking: `lexical`, `tie`, or `alphabet`;
- cycles: `cycle` or `deadlock`; and
- input preservation: `preserv` together with `input`, `job`, or `manifest`,
  or both `not` and `mutat`.

Every extracted test body must contain a real oracle after comments and
literal contents are ignored: an exact built-in `assert!`, `assert_eq!`,
`assert_ne!`, `debug_assert!`, `debug_assert_eq!`, `debug_assert_ne!`, or
`panic!` macro; an exact failing result call `.unwrap(...)`,
`.unwrap_err(...)`, `.expect(...)`, or `.expect_err(...)`; or `?` in a test
whose return type contains `Result`. Intervening whitespace and comments are
permitted. Fallback methods such as `.unwrap_or(...)`,
`.unwrap_or_default(...)`, and `.unwrap_or_else(...)`, similarly named
methods, standalone `matches!`, `.is_ok`, `.is_err`, custom assertion macros,
`todo!`, and `unreachable!` do not count unless nested in an accepted
assertion macro.

The suite must reference the exact identifiers `parse_manifest` and
`build_plan` through the absolute external-crate paths
`::release_plan::parse_manifest(...)` and
`::release_plan::build_plan(...)`; imported bare names do not count. Each
parsing or invalid-dependency topical test needs the former, and each
completed-prerequisite, priority, lexical, cycle, or input-preservation topical
test needs the latter. The required call must be coupled to its oracle: either
chain the call immediately to `.unwrap(...)`, `.unwrap_err(...)`,
`.expect(...)`, or `.expect_err(...)`, or place it in the first argument of an
accepted assertion macro. Merely naming or storing a function pointer, calling
a same-named local function, or placing an unrelated oracle elsewhere does not
count. Aliasing any macro to one of the accepted oracle names is forbidden in
candidate Rust source. A combined test must meet every applicable rule.

The built test binary is listed before execution: at least six distinct names
must actually be registered, and those registered names—not inactive source
text—must cover all seven topic stems above using ASCII-case-insensitive
matching. The executed passing count must equal the listed count.

Do not use `#[ignore]` or a disabling cfg. The only direct gating attribute
accepted in this file is exactly `#[cfg(test)]`. `cfg_attr` is inspected
recursively: it may not emit `ignore` or a disallowed `cfg` (a harmless output
such as `cfg_attr(test, should_panic)` is allowed). Ordinary identifiers named
`cfg` or `ignore`, and the expression macro `cfg!(test)`, remain allowed.

### Mechanical source-safety gate

Do not use the exact identifiers `include`, `include_str`, or `include_bytes`,
including through imports, aliases, or namespaced macro calls, and do not use
an outer `#[path = ...]` source-inclusion attribute (including one emitted by
`cfg_attr`). Do not define local macros with the exact identifier
`macro_rules`; this prevents a local macro from impersonating an accepted
built-in oracle. The exact identifiers `asm`, `global_asm`, and `naked_asm`
are also forbidden throughout candidate Rust source. The global lexical gate
likewise rejects the exact identifiers `Command`, `TcpStream`, `TcpListener`,
`UdpSocket`, `ToSocketAddrs`, `UnixStream`, `UnixDatagram`, `UnixListener`,
and `extern`, including when an otherwise unrelated user-defined item uses one
of those spellings.

Outside `src/main.rs`, the lexical gate rejects these exact identifiers even
when they are used as ordinary local names: `fs`, `env`, `thread`,
`backtrace`, `Path`, `path`, `read_link`, `read_dir`, `metadata`,
`symlink_metadata`, `canonicalize`, `exists`, `try_exists`, `is_file`,
`is_dir`, and `track_caller`. It also rejects the exact pair `Location` and
`caller` in one source file. Use task-specific names such as `plan`, `order`,
or `present` instead. These deliberately mechanical restrictions keep the
library and tests unable to identify or inspect their test runner;
`src/main.rs` alone may use filesystem and environment APIs for CLI argument
and manifest I/O. Outside the CLI, co-occurring exact identifiers `std`,
`process`, and `exit` are treated as process termination. Process-command
APIs, network APIs, FFI, unsafe Rust, and process termination outside the CLI
remain forbidden.

Before any Rust-source mutation, create `PLAN.md` with the exact headings
`## Contract`, `## File plan`, and `## Verification`. Its Markdown checklist
must contain a checked (`- [x]`) item naming each of `src/model.rs`,
`src/parser.rs`, `src/scheduler.rs`, `src/main.rs`, and
`tests/agent_tests.rs`, plus a checked item for the authorized test run. A
finished plan contains no unchecked (`- [ ]`) items.
