# Agent Tasks (Persistent Backlog)

- [ ] T-006 (sprint 0): Implement the symlink-safe, prefix-collision-proof workspace boundary in ferric-guard — touches: crates/ferric-guard/src/{lib.rs,workspace.rs}
- [ ] T-007 (sprint 0): Add the hardcoded permission checker and compile-time deny lists — touches: crates/ferric-guard/src/{checker.rs,denylist.rs}
- [ ] T-008 (sprint 0): Build the Tool trait, ToolSpec, and registry with a single execute chokepoint — touches: crates/ferric-tools/src/{lib.rs,spec.rs,registry.rs}
- [ ] T-009 (sprint 0): Implement builtin file tools read_file, write_file, list_dir resolving through the workspace boundary — touches: crates/ferric-tools/src/builtin/{mod.rs,read_file.rs,write_file.rs,list_dir.rs}
- [ ] T-010 (sprint 0): Build the ferric CLI stub: --version and trace cat derived view — touches: crates/ferric-cli/src/main.rs
- [ ] T-011 (sprint 0): Add GitHub Actions CI: fmt/clippy/test on windows+ubuntu plus aarch64-unknown-linux-gnu check gate — touches: .github/workflows/ci.yml
- [ ] T-012 (sprint 0): Record ADRs 001–009 in decisions.md and commit — touches: decisions.md
- [ ] T-013 (sprint 0): Create the public GitHub repo crussella0129/Animus_Ferric and push main with green CI — touches: git remote config only
