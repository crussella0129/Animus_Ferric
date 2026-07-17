# Refactoring Tasks (From Architecture Report)

## animus-launch
- [ ] Migrate `std::fs` to `tokio::fs` for async filesystem operations in project scaffolding.

## ferric-bench
- [ ] Decouple Validation from Traces in `src/verify.rs` by introducing an `Assertion` trait.

## ferric-cli
- [ ] Consolidate configuration precedence by merging `config.rs` and `backend.rs` into a unified `ConfigManager`.

## ferric-core
- [x] Isolate base64 encoding from `media.rs` into an optional module or crate feature (`media-encoding`).
- [x] Standardize errors in `error.rs` to derive `thiserror::Error`.

## ferric-guard
- [ ] Implement Dynamic Denylist Configuration in `src/denylist.rs` to read from a `.ferricignore` file.

## ferric-loop
- [x] Extract Loop State from `run.rs` into a dedicated `LoopState` struct.
- [x] Decouple Driver from Logic by breaking down the monolithic `while` loop into a `step(&mut LoopState) -> Result<TurnOutcome>` function.

## ferric-prompt
- [ ] Implement a templating engine (`minijinja` or `askama`) to replace manual `format!()` strings in `src/lib.rs`.

## ferric-provider
- [ ] Optimize SSE streaming in `src/stream_scan.rs` using `bytes::BytesMut` and `serde_json::StreamDeserializer` instead of `String` buffering.

## ferric-research
- [ ] Define a `Retriever` trait in `retriever.rs` to implement a plugin architecture for external system integration.
