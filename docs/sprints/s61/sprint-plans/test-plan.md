# Finalized - DO NOT EDIT

## Test Plan
1. Run `cargo test -p ferric-loop` to ensure our changes don't break the grammar tests.
2. Run `cargo build -p ferric-cli --features backend-openai`.
3. Start the test server `ferric server up`.
4. Run the demo query (`make a folder called 'test' in C:\Users\charl and in that make a file called 'script1' which contains a python script describing the fibonacci the nth number of the sequence, given n`).
5. Verify the generated code quality and inspect the trace to ensure the `"thought"` field is being populated.
