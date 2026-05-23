import { VoiceConfig } from "../types";
import { FieldGroup, Field } from "../components/ui";

export function PromptTab({
  config,
  setConfig,
  corePrompt,
  systemInfo,
}: {
  config: VoiceConfig;
  setConfig: (c: VoiceConfig) => void;
  corePrompt: string;
  systemInfo: string;
}) {
  return (
    <div className="flex flex-col gap-5 p-5 px-6 h-full">
      <FieldGroup title="Core System Prompt">
        <Field label="Read Only">
          <textarea
            value={corePrompt}
            readOnly
            rows={9}
            className="w-full bg-white/[0.025] border border-white/[0.06] text-white/45 px-3 py-2.5 rounded-lg text-[12px] font-mono outline-none resize-y min-h-[150px]"
          />
        </Field>
      </FieldGroup>

      <FieldGroup title="System Info (auto-generated)">
        <Field label="Read Only">
          <textarea
            value={systemInfo}
            readOnly
            rows={6}
            className="w-full bg-white/[0.025] border border-white/[0.06] text-white/45 px-3 py-2.5 rounded-lg text-[12px] font-mono outline-none resize-y min-h-[100px]"
          />
        </Field>
      </FieldGroup>

      <FieldGroup title="User Prompt">
        <Field label="Editable">
          <textarea
            value={config.system_prompt}
            onChange={(e) => setConfig({ ...config, system_prompt: e.target.value })}
            rows={14}
            className="w-full bg-white/[0.05] border border-white/10 text-white/90 px-3 py-2.5 rounded-lg text-[13px] font-inherit outline-none resize-y min-h-[300px] transition-all duration-200 focus:border-blue-500/50 focus:bg-white/[0.07] placeholder:text-white/20"
          />
        </Field>
      </FieldGroup>
    </div>
  );
}
