Finalized - DO NOT EDIT

# Sprint 59: Test Plan (`shell_exec` Tool)

1. **`ferric-guard` checks**
   - Unit tests to ensure `check_command` catches denied substrings like `rm -rf /` and allows normal commands.

2. **Timeout Verification**
   - Unit test executing a blocking script (e.g., `sleep 100` on unix or `timeout 100` on windows) to ensure `shell_exec` forcefully terminates it at 60s and returns a timeout error.

3. **Output Cap Verification**
   - Unit test executing a command that outputs massive text, verifying that the captured result string truncates strictly at 10,000 bytes and appends the `... [TRUNCATED]` suffix.

4. **Registry Enforcement**
   - Ensure a tool yielding a denied `target_command` fails safely at the `Registry::execute` gate before running.
