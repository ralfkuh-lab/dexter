import React, { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { VoiceConfig, SettingsTab } from "../types";
import { ConfigTab } from "./ConfigTab";
import { PromptTab } from "./PromptTab";
import { ToolsTab } from "./ToolsTab";
import { KnowledgeTab } from "./KnowledgeTab";

export function Settings() {
  const [config, setConfig] = useState<VoiceConfig | null>(null);
  const [corePrompt, setCorePrompt] = useState("");
  const [systemInfo, setSystemInfo] = useState("");
  const [saved, setSaved] = useState(false);
  const [tab, setTab] = useState<SettingsTab>("config");

  useEffect(() => {
    invoke<VoiceConfig>("get_config").then(setConfig);
    invoke<string>("get_core_system_prompt").then(setCorePrompt);
    invoke<string>("get_system_info").then(setSystemInfo);
  }, []);

  const save = async () => {
    if (!config) return;
    await invoke("set_config", { config });
    setSaved(true);
    setTimeout(() => setSaved(false), 1500);
  };

  if (!config) return null;

  const tabs: { id: SettingsTab; label: string }[] = [
    { id: "config", label: "Config" },
    { id: "prompt", label: "Prompt" },
    { id: "tools", label: "Tools" },
    { id: "knowledge", label: "Knowledge" },
  ];

  return (
    <div className="h-screen flex flex-col settings-bg backdrop-blur-xl overflow-hidden">
      {/* Header */}
      <div className="flex items-center justify-between px-6 pt-5 pb-3.5" style={{ WebkitAppRegion: "drag" } as React.CSSProperties}>
        <h2 className="text-base font-semibold text-white/85 tracking-tight">Settings</h2>
        {tab !== "knowledge" && (
          <button
            onClick={save}
            className="px-4 py-1.5 rounded-md text-[12px] font-medium border-none cursor-pointer bg-blue-500 text-white hover:bg-blue-600 transition-colors duration-150"
            style={{ WebkitAppRegion: "no-drag" } as React.CSSProperties}
          >
            {saved ? "Saved!" : "Save"}
          </button>
        )}
      </div>

      {/* Tab bar */}
      <div className="flex gap-0.5 px-6 border-b border-white/[0.08]">
        {tabs.map((t) => (
          <button
            key={t.id}
            onClick={() => setTab(t.id)}
            className={`px-4 py-2.5 text-[13px] font-medium border-none bg-transparent cursor-pointer -mb-px transition-colors duration-150 border-b-2 ${
              tab === t.id
                ? "text-white/90 border-b-blue-500"
                : "text-white/35 border-b-transparent hover:text-white/55"
            }`}
          >
            {t.label}
          </button>
        ))}
      </div>

      {/* Body */}
      <div className="flex-1 overflow-y-auto custom-scrollbar">
        {tab === "config" && <ConfigTab config={config} setConfig={setConfig} />}
        {tab === "prompt" && <PromptTab config={config} setConfig={setConfig} corePrompt={corePrompt} systemInfo={systemInfo} />}
        {tab === "tools" && <ToolsTab config={config} setConfig={setConfig} />}
        {tab === "knowledge" && <KnowledgeTab />}
      </div>
    </div>
  );
}
