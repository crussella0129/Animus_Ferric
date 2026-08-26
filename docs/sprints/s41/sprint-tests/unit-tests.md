# Sprint 41 Unit Tests (structural checks)

This sprint produces no Rust code — the "unit tests" are structural validations of the design
artifacts, run via Python (no `hadolint` in this environment). All pass.

## T-4102 — `docker/Dockerfile` structural checks
A Python script (`re`-based) confirmed all four:
- **Stage-label resolution:** the two stages `build`/`runtime` are declared via `FROM ... AS
  <name>`, and the single `COPY --from=build` resolves to the real earlier `build` stage.
- **`--features backend-openai` present** in the build stage (plan-critic C-001 — without it the
  compiled `ferric` cannot drive the co-located backend over the HTTP valve).
- **No `EXPOSE`** directive (loopback-only intent preserved structurally, ADR-005/ADR-051).
- Result: `ALL STRUCTURAL CHECKS PASS` (stages `['build','runtime']`, COPY target `['build']`,
  `--features backend-openai` True, EXPOSE False).

## T-4103 — `docker/docker-compose.yml` structural checks
A Python `yaml.safe_load` + assertion script confirmed:
- **YAML validity:** parses without error.
- **`ferric-core` is the only active service**, its `build.dockerfile` value is exactly
  `docker/Dockerfile` AND that path resolves to the real file created in T-4102 (plan-critic C-004
  — a concrete scriptable string+path match, not a vague "consistency" judgement).
- **No functional `ports:`** on `ferric-core` (plan-critic C-006).
- **`ornstein-search`/`chat` are commented STUBs** (present in the file text, absent from the
  parsed `services` map), with an explicit `STUB` marker.
- Result: `ALL STRUCTURAL CHECKS PASS`.

## Result
Both artifacts pass every structural check. These are the fast pre-checks; the authoritative
validation is the live Docker run (see `integration-tests.md`/`e2e-tests.md`, T-4105).
