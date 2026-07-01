export interface ProcessingState {
  stage: string;
  text: string;
}

export interface ToolsConfig {
  search_notes: boolean;
  screenshot: boolean;
  read_clipboard: boolean;
  open_url: boolean;
  get_current_time: boolean;
  list_apps: boolean;
  run_command: boolean;
  web_fetch: boolean;
  show_panel: boolean;
  ask_user: boolean;
}

export interface SandboxConfig {
  mode: "Guarded" | "Docker";
  timeout_secs: number;
  readable_paths: string[];
  workspace: string;
  docker_image: string;
  allow_network: boolean;
}

export interface WindowConfig {
  decorations: boolean;
  width: number;
  height: number;
  x: number | null;
  y: number | null;
}

export interface VoiceConfig {
  whisper_server_url: string;
  llm_provider: string;
  llm_base_url: string;
  llm_model: string;
  vault_path: string;
  vision_model: string;
  tts_url: string;
  tts_voice: string;
  debug_bubbles: boolean;
  system_prompt: string;
  tools: ToolsConfig;
  sandbox: SandboxConfig;
  window: WindowConfig;
  hotkey: string;
  dictation_hotkey: string;
  show_stats: boolean;
  tts_enabled: boolean;
}

export interface LlmStats {
  ttft_ms: number | null;
  tokens_per_sec: number | null;
  prompt_tokens: number | null;
  completion_tokens: number | null;
  ctx_max: number | null;
  model: string | null;
}

export interface AudioChunk {
  index: number;
  audio: string;
}

export interface DialogOption {
  label: string;
  description?: string | null;
}

export interface DialogPayload {
  question: string;
  options: DialogOption[];
}

export interface AgentDraftInfo {
  mode: string;
  content: string;
  spoken_log: string[];
  last_segment: string;
  status: string;
}

export interface ChatBubble {
  role: "user" | "assistant" | "status" | "tool" | "debug";
  text: string;
  id: number;
  detail?: string;
}

export interface DebugEvent {
  summary: string;
  detail: string;
}

export type SettingsTab = "config" | "prompt" | "tools" | "knowledge";

export const TOOL_LABEL_MAP: Record<string, string> = {
  take_screenshot: "Taking screenshot",
  search_notes: "Searching notes",
  read_note: "Reading note",
  read_clipboard: "Reading clipboard",
  open_url: "Opening URL",
  get_current_time: "Checking time",
  list_running_apps: "Listing apps",
  run_command: "Running command",
  web_fetch: "Fetching web page",
  show_panel: "Showing panel",
  ask_user: "Asking question",
};
