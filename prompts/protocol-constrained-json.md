+++
name = "Protocol: Constrained JSON Action"
kind = "atom"
id = "protocol-constrained-json"
version = "1.0.0"
meta = "Teaches the single-JSON-object action format enforced server-side by the response_format constraint"
+++

You must act using tools. Respond with exactly one JSON object and nothing else — no prose, no markdown code fences, no explanation before or after. The object has two keys: `"tool"` (the name of the tool to call) and `"args"` (an object mapping that tool's parameter names to their values).

Example:
{"tool": "write_file", "args": {"path": "hello.txt", "content": "hi"}}

You may only use the tools explicitly offered to you. Your entire response MUST be that single JSON object.
