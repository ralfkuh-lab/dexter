#!/usr/bin/env python3
"""Tool-Calling Eval: Testet ob das LLM korrekt Tools aufruft.

Sendet Szenarien aus scenarios.json direkt an den llama.cpp-Server
(OpenAI-kompatibles API) und bewertet die Antworten.

Nutzung:
    python3 tests/tool_calling/eval.py
    python3 tests/tool_calling/eval.py --system-prompt-file prompts/v2.md
    python3 tests/tool_calling/eval.py --base-url http://127.0.0.1:8081
    python3 tests/tool_calling/eval.py --verbose
"""

import argparse
import json
import sys
import time
from pathlib import Path
from urllib import request, error

SCRIPT_DIR = Path(__file__).parent
PROJECT_ROOT = SCRIPT_DIR.parent.parent
DEFAULT_SCENARIOS = SCRIPT_DIR / "scenarios.json"
DEFAULT_SYSTEM_PROMPT = PROJECT_ROOT / "system-prompt.md"
DEFAULT_BASE_URL = "http://127.0.0.1:8081"

# Tool-Definitionen — identisch zu voice/tool_defs.rs
TOOLS = [
    {
        "type": "function",
        "function": {
            "name": "get_current_time",
            "description": "Get the current date, time, and day of week.",
            "parameters": {"type": "object", "properties": {}},
        },
    },
    {
        "type": "function",
        "function": {
            "name": "read_clipboard",
            "description": "Read the current text contents of the user's clipboard.",
            "parameters": {"type": "object", "properties": {}},
        },
    },
    {
        "type": "function",
        "function": {
            "name": "take_screenshot",
            "description": "Capture a screenshot and describe what is visible.",
            "parameters": {
                "type": "object",
                "properties": {
                    "question": {"type": "string", "description": "What to look for"},
                },
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "run_command",
            "description": "Execute a shell command and return output.",
            "parameters": {
                "type": "object",
                "properties": {
                    "command": {"type": "string", "description": "Shell command"},
                },
                "required": ["command"],
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "open_url",
            "description": "Open a URL in the default browser.",
            "parameters": {
                "type": "object",
                "properties": {
                    "url": {"type": "string", "description": "URL to open"},
                },
                "required": ["url"],
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "list_running_apps",
            "description": "List currently running applications.",
            "parameters": {"type": "object", "properties": {}},
        },
    },
    {
        "type": "function",
        "function": {
            "name": "web_fetch",
            "description": "Fetch a web page and return its text content.",
            "parameters": {
                "type": "object",
                "properties": {
                    "url": {"type": "string", "description": "URL to fetch"},
                },
                "required": ["url"],
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "web_search",
            "description": "Search the web via the local metasearch engine. Use for current events, facts, prices, or anything outside your training data. Returns titles, URLs and snippets of the top results.",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "The search query."},
                    "max_results": {"type": "integer", "description": "Maximum number of results (default 5, max 8)."},
                },
                "required": ["query"],
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "show_panel",
            "description": "Open a detail panel with Markdown content.",
            "parameters": {
                "type": "object",
                "properties": {
                    "title": {"type": "string"},
                    "content": {"type": "string"},
                },
                "required": ["title", "content"],
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "ask_user",
            "description": "Ask the user to choose from options.",
            "parameters": {
                "type": "object",
                "properties": {
                    "question": {"type": "string"},
                    "options": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "label": {"type": "string"},
                                "description": {"type": "string"},
                            },
                            "required": ["label"],
                        },
                    },
                },
                "required": ["question", "options"],
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "switch_mode",
            "description": "Switch Dexter's application mode. Use when the user wants to start a coding session with a CLI agent (claude, codex, agy, opencode) or return to chat mode.",
            "parameters": {
                "type": "object",
                "properties": {
                    "mode": {
                        "type": "string",
                        "enum": [
                            "chat",
                            "claude_session",
                            "codex_session",
                            "agy_session",
                            "opencode_session",
                        ],
                    },
                },
                "required": ["mode"],
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "search_notes",
            "description": "Search the local Markdown notes vault.",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": {"type": "string"},
                },
                "required": ["query"],
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "read_note",
            "description": "Read one note from the local Markdown notes vault.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                },
                "required": ["path"],
            },
        },
    },
]


def load_system_prompt(path: Path) -> str:
    content = path.read_text()
    # Strip markdown header + separator
    if "\n---\n" in content:
        content = content.split("\n---\n", 1)[1]
    return content.strip()


def send_request(base_url: str, system_prompt: str, user_prompt: str) -> dict:
    body = json.dumps(
        {
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": user_prompt},
            ],
            "tools": TOOLS,
            "temperature": 0,
            "max_tokens": 512,
            "stream": False,
            "chat_template_kwargs": {"enable_thinking": False},
        }
    ).encode()

    req = request.Request(
        f"{base_url}/v1/chat/completions",
        data=body,
        headers={"Content-Type": "application/json"},
        method="POST",
    )

    try:
        with request.urlopen(req, timeout=60) as resp:
            return json.loads(resp.read())
    except error.URLError as e:
        return {"error": str(e)}


def evaluate_response(response: dict, scenario: dict) -> tuple[str, str]:
    """Returns (status, detail)."""
    if "error" in response:
        return "ERROR", response["error"]

    choices = response.get("choices", [])
    if not choices:
        return "ERROR", "No choices in response"

    message = choices[0].get("message", {})
    tool_calls = message.get("tool_calls", None)
    content = message.get("content", "")
    expected = scenario.get("expected_tool")

    if expected is None:
        # Should NOT call a tool
        if tool_calls:
            tool_name = tool_calls[0]["function"]["name"]
            return "UNWANTED_TOOL", f"Called {tool_name} but expected text-only"
        return "PASS", f"Text: {content[:80]}"

    # Should call a tool
    if not tool_calls:
        # Check if the text contains a tool-call-like pattern (rescue candidate)
        rescue_patterns = [
            f"call {expected}",
            f"{expected}(",
            f'"tool": "{expected}"',
        ]
        for p in rescue_patterns:
            if p in content.lower():
                return "TEXT_INSTEAD", f"Text contains '{p}' but no actual tool call: {content[:120]}"
        return "TEXT_INSTEAD", f"No tool call. Text: {content[:120]}"

    called_name = tool_calls[0]["function"]["name"]
    if called_name != expected:
        return "WRONG_TOOL", f"Called {called_name}, expected {expected}"

    # Check args if specified
    expected_args = scenario.get("expected_args")
    if expected_args:
        try:
            actual_args = json.loads(tool_calls[0]["function"]["arguments"])
        except (json.JSONDecodeError, KeyError):
            actual_args = {}
        for key, value in expected_args.items():
            if actual_args.get(key) != value:
                return "WRONG_ARGS", f"Arg {key}={actual_args.get(key)!r}, expected {value!r}"

    return "PASS", f"Correct tool call: {called_name}"


def main():
    parser = argparse.ArgumentParser(description="Tool-Calling Eval")
    parser.add_argument(
        "--system-prompt-file",
        type=Path,
        default=DEFAULT_SYSTEM_PROMPT,
        help="Path to system prompt file",
    )
    parser.add_argument(
        "--scenarios",
        type=Path,
        default=DEFAULT_SCENARIOS,
        help="Path to scenarios JSON",
    )
    parser.add_argument(
        "--base-url",
        default=DEFAULT_BASE_URL,
        help="LLM server base URL",
    )
    parser.add_argument("--verbose", "-v", action="store_true")
    parser.add_argument(
        "--scenario",
        "-s",
        help="Run only this scenario ID",
    )
    args = parser.parse_args()

    system_prompt = load_system_prompt(args.system_prompt_file)
    scenarios = json.loads(args.scenarios.read_text())

    if args.scenario:
        scenarios = [s for s in scenarios if s["id"] == args.scenario]
        if not scenarios:
            print(f"Scenario '{args.scenario}' not found")
            sys.exit(1)

    print(f"System prompt: {args.system_prompt_file} ({len(system_prompt)} chars)")
    print(f"Scenarios: {len(scenarios)}")
    print(f"Server: {args.base_url}")
    print()

    results = {"PASS": 0, "TEXT_INSTEAD": 0, "WRONG_TOOL": 0, "WRONG_ARGS": 0, "UNWANTED_TOOL": 0, "ERROR": 0}

    for scenario in scenarios:
        sid = scenario["id"]
        prompt = scenario["prompt"]

        t0 = time.time()
        response = send_request(args.base_url, system_prompt, prompt)
        elapsed = time.time() - t0

        status, detail = evaluate_response(response, scenario)
        results[status] = results.get(status, 0) + 1

        icon = "✓" if status == "PASS" else "✗"
        color = "\033[92m" if status == "PASS" else "\033[91m"
        reset = "\033[0m"
        print(f"  {color}{icon}{reset} [{sid}] {status} ({elapsed:.1f}s)")
        if args.verbose or status != "PASS":
            print(f"    Prompt: {prompt}")
            print(f"    {detail}")
            print()

    total = len(scenarios)
    passed = results["PASS"]
    print(f"\n{'='*50}")
    print(f"Score: {passed}/{total} ({100*passed/total:.0f}%)")
    for status, count in sorted(results.items()):
        if count > 0:
            print(f"  {status}: {count}")

    sys.exit(0 if passed == total else 1)


if __name__ == "__main__":
    main()
