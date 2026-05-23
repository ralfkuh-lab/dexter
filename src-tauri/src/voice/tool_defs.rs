//! Tool-Definitionen für den LLM-Funktionsaufruf — gefiltert nach
//! `ToolsConfig`-Toggles und plattformabhängiger Shell.

use crate::ToolsConfig;

/// Build tool definitions based on enabled tools in config.
pub fn build_tools(tools_config: &ToolsConfig) -> Vec<serde_json::Value> {
    let shell_name = if cfg!(target_os = "macos") {
        "zsh"
    } else if cfg!(target_os = "windows") {
        "PowerShell"
    } else {
        "sh"
    };

    let mut tools = Vec::new();

    if tools_config.search_knowledge {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "search_knowledge",
                "description": "Search the user's local knowledge base for relevant information. Use this when the user asks about something that might be in their stored documents or notes.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "The search query to find relevant knowledge"
                        }
                    },
                    "required": ["query"]
                }
            }
        }));
    }

    if tools_config.screenshot {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "take_screenshot",
                "description": "Capture a screenshot of the user's screen and describe what is visible. Use this when the user asks what's on their screen, asks you to look at something, or wants help with something they're looking at. By default captures the active monitor (where the mouse cursor is).",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "question": {
                            "type": "string",
                            "description": "What to look for or describe in the screenshot. Defaults to a general description."
                        },
                        "monitor": {
                            "type": "integer",
                            "description": "Which monitor to capture (1 = primary, 2 = secondary, etc). If omitted, captures the active monitor where the mouse cursor is."
                        }
                    }
                }
            }
        }));
    }

    if tools_config.read_clipboard {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "read_clipboard",
                "description": "Read the current text contents of the user's clipboard. Use this when the user says they copied something, or asks about what's in their clipboard. The clipboard changes constantly — ALWAYS call this fresh every time it is referenced; never reuse a previous result from earlier in the conversation, even if you just called it moments ago.",
                "parameters": {
                    "type": "object",
                    "properties": {}
                }
            }
        }));
    }

    if tools_config.open_url {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "open_url",
                "description": "Open a URL in the user's default web browser. Use when the user asks to open a website, search something on the web, or navigate to a URL.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "url": {
                            "type": "string",
                            "description": "The URL to open"
                        }
                    },
                    "required": ["url"]
                }
            }
        }));
    }

    if tools_config.get_current_time {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "get_current_time",
                "description": "Get the current date, time, and day of week. Use when the user asks what time or date it is. Time advances continuously — ALWAYS call this fresh every time the user asks; never reuse a previous result, even if you just answered a time question seconds ago.",
                "parameters": {
                    "type": "object",
                    "properties": {}
                }
            }
        }));
    }

    if tools_config.list_apps {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "list_running_apps",
                "description": "List the user's currently running applications or open windows. Use when the user asks what apps are open or running.",
                "parameters": {
                    "type": "object",
                    "properties": {}
                }
            }
        }));
    }

    if tools_config.web_fetch {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "web_fetch",
                "description": "Fetch a web page and return its text content. Use when the user asks about something online, wants you to read an article, check a website, look up documentation, or get current information from the web.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "url": {
                            "type": "string",
                            "description": "The URL to fetch"
                        }
                    },
                    "required": ["url"]
                }
            }
        }));
    }

    if tools_config.show_panel {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "show_panel",
                "description": "Open a separate detail panel with Markdown-formatted content that is too long or complex for speech. Use for file listings, code, tables, diffs, build output, and detailed results. Always still speak a short summary. Calling again replaces the panel content.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "title": {
                            "type": "string",
                            "description": "Panel window title"
                        },
                        "content": {
                            "type": "string",
                            "description": "Markdown-formatted content to render in the panel"
                        }
                    },
                    "required": ["title", "content"]
                }
            }
        }));
    }

    if tools_config.ask_user {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "ask_user",
                "description": "Ask the user to choose from a short list of options when you need clarification before continuing. The choice appears in the orb and can be answered by click or voice. Use 2 to 4 concise options.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "question": {
                            "type": "string",
                            "description": "The clarification question to ask"
                        },
                        "options": {
                            "type": "array",
                            "description": "Two to four answer options",
                            "minItems": 2,
                            "maxItems": 4,
                            "items": {
                                "type": "object",
                                "properties": {
                                    "label": {
                                        "type": "string",
                                        "description": "Short option label"
                                    },
                                    "description": {
                                        "type": "string",
                                        "description": "Optional short explanation"
                                    }
                                },
                                "required": ["label"]
                            }
                        }
                    },
                    "required": ["question", "options"]
                }
            }
        }));
    }

    if tools_config.run_command {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "run_command",
                "description": format!("Execute a shell command on the user's computer and return its output. Use when the user asks to check system status, manage files, run scripts, install something, or perform any task that requires terminal access. Always prefer specific, minimal commands. The command runs in {}.", shell_name),
                "parameters": {
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": format!("The shell command to execute (runs in {})", shell_name)
                        }
                    },
                    "required": ["command"]
                }
            }
        }));
    }

    if tools_config.switch_mode {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "switch_mode",
                "description": "Switch Dexter's application mode. Use when the user wants to start a coding session with a CLI agent (claude, codex, agy, opencode) or return to normal chat mode. This opens the agent in a visible terminal window.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "mode": {
                            "type": "string",
                            "enum": ["chat", "claude_session", "codex_session", "agy_session", "opencode_session"],
                            "description": "The mode to switch to. 'chat' for normal conversation, '*_session' to route voice to that CLI agent."
                        }
                    },
                    "required": ["mode"]
                }
            }
        }));
    }

    tools
}
