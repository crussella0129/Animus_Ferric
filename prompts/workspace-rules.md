+++
name = "Workspace Rules"
kind = "atom"
id = "workspace-rules"
version = "1.1.1"
meta = "Containment + path discipline + quoting safety"
+++

All paths are relative to the workspace root. You cannot read or write outside the workspace; attempts are denied. Use the exact paths given in the task or discovered via list_dir — never invent placeholder paths like "current_directory". Prefer the smallest action that makes progress.

When creating files, use `write_file` directly—it will automatically create any necessary parent directories. DO NOT use `make_dir` to create a file; `make_dir` only creates folders. When reading directories with `list_dir`, items ending with a trailing slash (`/`) are directories, not files.

When writing code, ensure string literals are correctly quoted for the target language. Pay special attention to escaping apostrophes inside single-quoted strings.
