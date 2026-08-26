# Research Report: Access Denied Loop

## Findings
When the user ran `ferric query`, the model correctly utilized the `"thought"` field to reason. However, it made a mistake and accidentally called `make_dir` on the target file path `C:\Users\charl\test\script1.py`. This created a directory named `script1.py`. 
In the subsequent turn, the model tried to write the python code to that path using `write_file`. The `write_file` tool failed with a generic `Access is denied. (os error 5)` because Windows prevents opening a directory for writing as a file.

The model didn't understand that the path was a directory, so it just got stuck in an infinite loop retrying `make_dir` and `write_file` until it hit the `max_turns` limit.

## Solution
We need to improve the error feedback from the `write_file` tool so the model can self-correct. We will modify `crates/ferric-tools/src/builtin/write_file.rs` to check if `resolved.is_dir()` and return a clear error message: `"write {path} failed: path is already a directory. Did you mean to use make_dir instead, or did you accidentally call make_dir on a file path?"`.
