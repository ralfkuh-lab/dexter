import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { VoiceConfig } from "../types";
import { FieldGroup, Field, Input, Toggle } from "../components/ui";
import { ModelSelect } from "../components/ModelSelect";

export function ConfigTab({
  config,
  setConfig,
}: {
  config: VoiceConfig;
  setConfig: (c: VoiceConfig) => void;
}) {
  const [inputDevices, setInputDevices] = useState<string[]>([]);

  useEffect(() => {
    invoke<string[]>("list_input_devices")
      .then(setInputDevices)
      .catch(() => setInputDevices([]));
  }, []);

  const unavailableInputDevice =
    config.input_device !== "" && !inputDevices.includes(config.input_device);

  return (
    <div className="flex flex-col gap-5 p-5 px-6">
      <FieldGroup title="Speech Recognition">
        <Field label="Mikrofon">
          <div className="relative">
            <select
              value={config.input_device}
              onChange={(e) => setConfig({ ...config, input_device: e.target.value })}
              style={{ colorScheme: "dark" }}
              className="appearance-none w-full bg-white/[0.05] border border-white/10 text-white/90 pl-3 pr-9 py-2.5 rounded-lg text-[13px] outline-none transition-all duration-200 focus:border-blue-500/50 focus:bg-white/[0.07] cursor-pointer"
            >
              <option value="" className="bg-neutral-800 text-white/90">
                System-Default
              </option>
              {unavailableInputDevice && (
                <option
                  value={config.input_device}
                  className="bg-neutral-800 text-white/90"
                >
                  {config.input_device} (nicht verfügbar)
                </option>
              )}
              {inputDevices.map((device) => (
                <option
                  key={device}
                  value={device}
                  className="bg-neutral-800 text-white/90"
                >
                  {device}
                </option>
              ))}
            </select>
            <span className="pointer-events-none absolute right-3 top-1/2 -translate-y-1/2 text-white/40 text-[10px]">
              ▼
            </span>
          </div>
        </Field>
        <Field label="Whisper Server URL">
          <Input value={config.whisper_server_url} onChange={(v) => setConfig({ ...config, whisper_server_url: v })} placeholder="http://127.0.0.1:8350" />
        </Field>
      </FieldGroup>

      <FieldGroup title="Language Model">
        <Field label="Provider">
          <Input value={config.llm_provider} onChange={(v) => setConfig({ ...config, llm_provider: v })} placeholder="openai" />
        </Field>
        <Field label="LLM Base URL">
          <Input value={config.llm_base_url} onChange={(v) => setConfig({ ...config, llm_base_url: v })} placeholder="http://127.0.0.1:8081" />
        </Field>
        <Field label="Chat Model">
          <ModelSelect
            value={config.llm_model}
            onChange={(v) => setConfig({ ...config, llm_model: v })}
            baseUrl={config.llm_base_url}
          />
        </Field>
      </FieldGroup>

      <FieldGroup title="Text-to-Speech">
        <Field label="TTS Server URL">
          <Input value={config.tts_url} onChange={(v) => setConfig({ ...config, tts_url: v })} placeholder="http://127.0.0.1:8005" />
        </Field>
        <Field label="Voice">
          <Input value={config.tts_voice} onChange={(v) => setConfig({ ...config, tts_voice: v })} placeholder="de_DE-thorsten-medium" />
        </Field>
      </FieldGroup>

      <FieldGroup title="Window">
        <Field label="Show title bar (decorations)">
          <Toggle
            on={config.window.decorations}
            onToggle={() =>
              setConfig({
                ...config,
                window: { ...config.window, decorations: !config.window.decorations },
              })
            }
          />
        </Field>
      </FieldGroup>

      <FieldGroup title="Agent Sessions">
        <Field label="Terminal">
          <Input
            value={config.terminal_command}
            onChange={(v) => setConfig({ ...config, terminal_command: v })}
            placeholder="gnome-terminal"
          />
        </Field>
      </FieldGroup>

      <FieldGroup title="Push-to-Talk">
        <Field label="Hotkey (hold to talk)">
          <Input
            value={config.hotkey}
            onChange={(v) => setConfig({ ...config, hotkey: v })}
            placeholder="F9"
          />
        </Field>
        <Field label="Dictation toggle hotkey">
          <Input
            value={config.dictation_hotkey}
            onChange={(v) => setConfig({ ...config, dictation_hotkey: v })}
            placeholder="F10"
          />
        </Field>
        <div className="text-[11px] text-white/40 leading-snug -mt-1">
          Tauri-Accelerator-Syntax, z.B. <code>F9</code>, <code>Super+F9</code>,{" "}
          <code>Control+Alt+Space</code>. Funktionstasten und nicht-textproduzierende
          Kombinationen vermeiden, dass die Taste zusätzlich ins fokussierte
          Textfeld rutscht.
        </div>
      </FieldGroup>

      <FieldGroup title="Debug">
        <Field label="Debug Bubbles">
          <Toggle on={config.debug_bubbles} onToggle={() => setConfig({ ...config, debug_bubbles: !config.debug_bubbles })} />
        </Field>
        <Field label="Stats bar (ctx, TTFT, tok/s)">
          <Toggle on={config.show_stats} onToggle={() => setConfig({ ...config, show_stats: !config.show_stats })} />
        </Field>
      </FieldGroup>
    </div>
  );
}
