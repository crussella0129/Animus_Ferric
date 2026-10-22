"""Drive one real Hermes CLI session for Ferric's valve E2E runner (T-12608).

Mirrors Animus Amalgam's ``evals/local_qualification/driver.py``. Hermes is
imported from the Amalgam checkout through this process's own ``sys.path``
only, and the session runs inside a disposable fixture and ``HERMES_HOME``
that the runner owns. The runner, not this script, judges completion. This
script only records what Hermes did.
"""

import argparse
import json
import os
import sys
import threading
import time
from pathlib import Path


def tool_call_validity(conversation, known_tools):
    """Per tool call: arguments parse as a JSON object and the tool exists."""
    calls = []
    for message in conversation:
        for call in message.get("tool_calls") or []:
            function = call.get("function", {})
            try:
                arguments_valid = isinstance(
                    json.loads(function.get("arguments") or "{}"), dict
                )
            except ValueError:
                arguments_valid = False
            calls.append(
                {
                    "name": function.get("name"),
                    "arguments_valid": arguments_valid,
                    "known_tool": function.get("name") in known_tools,
                }
            )
    return calls


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--hermes", type=Path, required=True, help="Amalgam checkout")
    parser.add_argument("--fixture", type=Path, required=True)
    parser.add_argument("--url", required=True, help="OpenAI-compatible base URL")
    parser.add_argument(
        "--prompts", type=Path, required=True, help="JSON list of user turns"
    )
    parser.add_argument("--toolsets", default="file")
    parser.add_argument("--max-turns", type=int, required=True)
    parser.add_argument("--max-tokens", type=int, required=True)
    parser.add_argument("--result", type=Path, required=True)
    parser.add_argument("--interrupt-file", type=Path, required=True)
    args = parser.parse_args()

    prompts = json.loads(args.prompts.read_text(encoding="utf-8"))
    sys.path.insert(0, str(args.hermes.resolve()))
    os.chdir(args.fixture)
    from cli import HermesCLI

    started = time.monotonic()
    toolsets = [name for name in args.toolsets.split(",") if name]
    cli = HermesCLI(
        model="amalgam-pilot",
        provider="custom",
        base_url=args.url,
        api_key="local-pilot",
        toolsets=toolsets,
        reasoning="none",
        max_turns=args.max_turns,
        ignore_rules=True,
    )
    cli._single_query_mode = True
    if not cli._init_agent():
        raise RuntimeError("Hermes CLI agent initialization failed")
    cli.agent.max_tokens = args.max_tokens
    ready = time.monotonic() - started

    def watch_interrupt():
        while not args.interrupt_file.exists():
            time.sleep(0.1)
        cli.agent.interrupt(
            hard_cancel=True, tool_reason="ferric valve e2e cancellation"
        )

    threading.Thread(target=watch_interrupt, daemon=True).start()
    responses = []
    error = None
    try:
        for prompt in prompts:
            if args.interrupt_file.exists():
                break
            responses.append(cli.chat(prompt))
    except Exception as exc:  # recorded, not hidden: the runner classifies it
        error = f"{type(exc).__name__}: {exc}"
    known = {tool["function"]["name"] for tool in (cli.agent.tools or [])}
    result = {
        "session_id": cli.session_id,
        "cli_start_seconds": ready,
        "elapsed_seconds": time.monotonic() - started,
        "responses": responses,
        "conversation": cli.conversation_history,
        "tool_calls": tool_call_validity(cli.conversation_history, known),
        "known_tools": sorted(known),
        "error": error,
    }
    args.result.write_text(json.dumps(result, indent=2, default=str), encoding="utf-8")


if __name__ == "__main__":
    main()
