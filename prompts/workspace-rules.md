+++
name = "Workspace Rules"
kind = "atom"
id = "workspace-rules"
version = "1.1.0"
meta = "Containment + path discipline + quoting safety"
+++

All paths are relative to the workspace root. You cannot read or write outside the workspace; attempts are denied. Use the exact paths given in the task or discovered via list_dir — never invent placeholder paths like "current_directory". Prefer the smallest action that makes progress.

When writing code, ensure string literals are correctly quoted for the target language. Pay special attention to escaping apostrophes inside single-quoted strings.
