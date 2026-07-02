import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Input } from "./ui";

export function ModelSelect({
  value,
  onChange,
  baseUrl,
}: {
  value: string;
  onChange: (v: string) => void;
  baseUrl: string;
}) {
  const [models, setModels] = useState<string[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = () => {
    setError(null);
    setModels(null);
    invoke<string[]>("list_models", { baseUrl })
      .then((m) => setModels(m))
      .catch((e) => {
        setModels([]);
        setError(String(e));
      });
  };

  useEffect(() => { refresh(); }, [baseUrl]);

  if (models === null) {
    return <div className="text-[12px] text-white/40 px-1">Loading models...</div>;
  }

  // Empty list or error → fall back to free-text input plus a hint.
  if (models.length === 0) {
    return (
      <div className="flex flex-col gap-1">
        <Input value={value} onChange={onChange} placeholder="model id" />
        <div className="text-[10px] text-white/35">
          {error ? `No models available (${error})` : "Endpoint returned no models"}
        </div>
      </div>
    );
  }

  // Selected value not in the list — append it so the user doesn't lose it.
  const options = models.includes(value) ? models : [...models, value].filter(Boolean);

  return (
    <div className="flex gap-2 items-center">
      <div className="relative flex-1">
        <select
          value={value}
          onChange={(e) => onChange(e.target.value)}
          style={{ colorScheme: "dark" }}
          className="appearance-none w-full bg-white/[0.05] border border-white/10 text-white/90 pl-3 pr-9 py-2.5 rounded-lg text-[13px] outline-none transition-all duration-200 focus:border-blue-500/50 focus:bg-white/[0.07] cursor-pointer"
        >
          {options.map((m) => (
            <option key={m} value={m} className="bg-neutral-800 text-white/90">{m}</option>
          ))}
        </select>
        <span className="pointer-events-none absolute right-3 top-1/2 -translate-y-1/2 text-white/40 text-[10px]">
          ▼
        </span>
      </div>
      <button
        onClick={refresh}
        title="Reload model list"
        className="text-white/50 hover:text-white/90 text-[12px] px-2 py-2 rounded-lg border border-white/10 hover:border-white/30 bg-white/[0.05]"
      >
        ↻
      </button>
    </div>
  );
}
