+++
name = "Workspace Rules"
kind = "atom"
id = "workspace-rules"
version = "1.0.0"
meta = "Containment + path discipline"
+++

All paths are relative to the workspace root. You cannot read or write outside the workspace; attempts are denied. Use the exact paths given in the task or discovered via list_dir — never invent placeholder paths like "current_directory". Prefer the smallest action that makes progress.
