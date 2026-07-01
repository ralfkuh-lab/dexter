import { VoiceConfig, ToolsConfig, SandboxConfig } from "../types";
import { FieldGroup, Field, Input, Toggle } from "../components/ui";

const TOOL_DEFINITIONS: { key: keyof ToolsConfig; name: string; desc: string; icon: string }[] = [
  { key: "screenshot", name: "Screenshot", desc: "Capture and describe what's on your screen", icon: "📸" },
  { key: "read_clipboard", name: "Read Clipboard", desc: "Read current text from your clipboard", icon: "📋" },
  { key: "search_knowledge", name: "Knowledge Search", desc: "Search your local knowledge base for context", icon: "🔍" },
  { key: "open_url", name: "Open URL", desc: "Open websites in your default browser", icon: "🌐" },
  { key: "get_current_time", name: "Current Time", desc: "Get the current date, time, and day of week", icon: "🕐" },
  { key: "list_apps", name: "Running Apps", desc: "List currently running applications", icon: "🖥" },
  { key: "web_fetch", name: "Web Fetch", desc: "Fetch and read web pages for information", icon: "🕸" },
  { key: "show_panel", name: "Detail Panel", desc: "Show long output, code, tables, and diffs in a separate window", icon: "🧾" },
  { key: "ask_user", name: "Ask User", desc: "Ask multiple-choice clarification questions in the orb", icon: "❓" },
  { key: "run_command", name: "Shell Command", desc: "Execute terminal commands in the platform shell", icon: "⚡" },
];

export function ToolsTab({
  config,
  setConfig,
}: {
  config: VoiceConfig;
  setConfig: (c: VoiceConfig) => void;
}) {
  const toggleTool = (key: keyof ToolsConfig) => {
    setConfig({ ...config, tools: { ...config.tools, [key]: !config.tools[key] } });
  };

  const setSandbox = (patch: Partial<SandboxConfig>) => {
    setConfig({ ...config, sandbox: { ...config.sandbox, ...patch } });
  };

  const enabledCount = TOOL_DEFINITIONS.filter((t) => config.tools[t.key]).length;

  return (
    <div className="flex flex-col gap-5 p-5 px-6">
      <p className="text-[13px] text-white/40 leading-relaxed flex items-center gap-2">
        Enable or disable tools the assistant can use.
        <span className="text-[11px] text-cyan-400/60 bg-cyan-400/[0.08] px-2 py-0.5 rounded">
          {enabledCount}/{TOOL_DEFINITIONS.length} active
        </span>
      </p>

      <div className="flex flex-col gap-1">
        {TOOL_DEFINITIONS.map((tool) => {
          const enabled = config.tools[tool.key];
          return (
            <div
              key={tool.key}
              className={`flex items-center gap-3 px-4 py-3.5 rounded-xl border transition-all duration-200 ${
                enabled
                  ? "bg-white/[0.03] border-white/[0.06]"
                  : "bg-white/[0.01] border-white/[0.03] opacity-50"
              }`}
            >
              <span className="text-xl w-8 text-center shrink-0">{tool.icon}</span>
              <div className="flex-1 min-w-0">
                <div className="text-[13px] font-medium text-white/85">{tool.name}</div>
                <div className="text-[11px] text-white/30 leading-relaxed mt-0.5">{tool.desc}</div>
              </div>
              <Toggle on={enabled} onToggle={() => toggleTool(tool.key)} />
            </div>
          );
        })}
      </div>

      {config.tools.run_command && (
        <FieldGroup title="Shell Sandbox">
          <p className="text-[12px] text-white/30 leading-relaxed -mt-1">
            Commands are validated, environment sanitized, and all executions logged.
          </p>

          <div className="flex gap-1 bg-white/[0.04] rounded-lg p-0.5">
            {(["Guarded", "Docker"] as const).map((mode) => (
              <button
                key={mode}
                onClick={() => setSandbox({ mode })}
                className={`flex-1 py-2 px-3 rounded-md text-[13px] font-medium border-none cursor-pointer transition-all duration-200 ${
                  config.sandbox.mode === mode
                    ? "bg-blue-500/30 text-white/90"
                    : "bg-transparent text-white/40 hover:text-white/60"
                }`}
              >
                {mode}
              </button>
            ))}
          </div>

          <p className="text-[11px] text-white/25 leading-relaxed">
            {config.sandbox.mode === "Guarded"
              ? "Isolated workspace, sanitized env, blocked dangerous commands."
              : "Docker container with memory/CPU limits, read-only root. Requires Docker Desktop."}
          </p>

          <Field label="Workspace Directory">
            <Input value={config.sandbox.workspace} onChange={(v) => setSandbox({ workspace: v })} />
          </Field>
          <Field label="Timeout (seconds)">
            <input
              type="number"
              value={config.sandbox.timeout_secs}
              onChange={(e) => setSandbox({ timeout_secs: parseInt(e.target.value) || 30 })}
              className="w-full bg-white/[0.05] border border-white/10 text-white/90 px-3 py-2.5 rounded-lg text-[13px] outline-none transition-all duration-200 focus:border-blue-500/50 focus:bg-white/[0.07]"
            />
          </Field>

          <Field label="Readable Paths (Datei-Ingest & Docker-Mounts)">
            <textarea
              value={config.sandbox.readable_paths.join("\n")}
              onChange={(e) => setSandbox({ readable_paths: e.target.value.split("\n").filter(Boolean) })}
              rows={3}
              placeholder={"~/Documents\n~/Desktop\n~/Downloads"}
              className="w-full bg-white/[0.05] border border-white/10 text-white/90 px-3 py-2.5 rounded-lg text-[13px] font-inherit outline-none resize-y transition-all duration-200 focus:border-blue-500/50 focus:bg-white/[0.07] placeholder:text-white/20"
            />
            <p className="text-[11px] text-white/25 mt-1 leading-relaxed">
              Nur Dateien unter diesen Pfaden dürfen in die Wissensbasis
              eingelesen werden. Im Docker-Modus werden sie zusätzlich read-only
              in den Container gemountet. Ein Pfad pro Zeile, `~` = Home.
            </p>
          </Field>

          {config.sandbox.mode === "Docker" && (
            <>
              <Field label="Docker Image">
                <Input value={config.sandbox.docker_image} onChange={(v) => setSandbox({ docker_image: v })} />
              </Field>
              <div className="flex items-center gap-3 px-4 py-3 rounded-xl bg-white/[0.02] border border-white/[0.04]">
                <div className="flex-1">
                  <div className="text-[13px] font-medium text-white/80">Allow Network</div>
                  <div className="text-[11px] text-white/30 mt-0.5">Let commands access the internet</div>
                </div>
                <Toggle on={config.sandbox.allow_network} onToggle={() => setSandbox({ allow_network: !config.sandbox.allow_network })} />
              </div>
            </>
          )}
        </FieldGroup>
      )}
    </div>
  );
}
