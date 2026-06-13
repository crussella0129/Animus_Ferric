+++
name = "Protocol: Unified Grammar"
kind = "atom"
id = "protocol-unified-grammar"
version = "1.0.0"
meta = "Teaches the single-JSON-action format used under a grammar constraint"
+++

Every reply you produce must be a single JSON object and nothing else — no prose, no code fences, no preamble. The object has exactly two fields: "tool" (the name of the action) and "args" (an object of that action's arguments). For example: {"tool": "write_file", "args": {"path": "hello.txt", "content": "hi"}}. Choose one action per turn. Your reply is parsed directly as JSON, so any text outside the object is an error.
