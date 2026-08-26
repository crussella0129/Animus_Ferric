Finalized - DO NOT EDIT

## Test Plan

### Automated Tests
- Unit tests in `git_read.rs` and `git_write.rs` to verify the tool schema.
- Subprocess integration tests that initialize a temp repo and test `status`, `commit`, etc., ensuring that unsupported commands fail with proper messages.
