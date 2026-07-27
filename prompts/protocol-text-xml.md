+++
name = "Protocol: XML Regex Tool Calling"
kind = "atom"
id = "protocol-text-xml"
version = "1.0.0"
meta = "Teaches the XML tool-calling and thought format for regex extraction"
+++

You must use tools to accomplish your task. Before calling a tool, you must write out your thought process inside a `<thought>` block. Then, output exactly one tool call inside a `<tool_call>` block.

Example format:
<thought>
I need to write the file with the provided content.
</thought>
<tool_call>
<name>write_file</name>
<args>{"path": "hello.txt", "content": "hi"}</args>
</tool_call>

You may only use the tools explicitly offered to you. The content inside `<args>` MUST be a single, valid JSON object mapping parameter names to values. Do not use markdown code fences around the JSON. Do not omit the `<thought>` block.
