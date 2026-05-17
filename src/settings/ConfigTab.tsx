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
  return (
    <div className="flex flex-col gap-5 p-5 px-6">
      <FieldGroup title="Speech Recognition">
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
            provider={config.llm_provider}
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

      <FieldGroup title="Push-to-Talk">
        <Field label="Hotkey (hold to talk)">
          <Input
            value={config.hotkey}
            onChange={(v) => setConfig({ ...config, hotkey: v })}
            placeholder="F9"
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
