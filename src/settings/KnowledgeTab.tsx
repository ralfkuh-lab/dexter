import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { VoiceConfig } from "../types";
import { FieldGroup, Field, Input } from "../components/ui";

export function KnowledgeTab({
  config,
  setConfig,
}: {
  config: VoiceConfig;
  setConfig: (c: VoiceConfig) => void;
}) {
  const [notes, setNotes] = useState<string[]>([]);
  const [status, setStatus] = useState("");

  const loadNotes = async () => {
    try {
      const result = await invoke<string[]>("list_vault_notes");
      setNotes(result);
      setStatus(`${result.length} Markdown-Notiz(en) im Vault gefunden.`);
    } catch (e) {
      setNotes([]);
      setStatus(`Fehler: ${e}`);
    }
  };

  useEffect(() => {
    loadNotes();
  }, []);

  return (
    <div className="flex flex-col gap-5 p-5 px-6">
      <p className="text-[13px] text-white/40 leading-relaxed">
        Wissensbasis als Ordner mit Markdown-Dateien (Obsidian-kompatibel). Der
        Assistent durchsucht sie per <code>search_notes</code> und liest Dateien
        per <code>read_note</code>. Keine Embeddings, keine Datenbank — die
        Dateien sind auch außerhalb von Dexter editierbar.
      </p>

      <FieldGroup title="Vault-Ordner">
        <Field label="Pfad">
          <Input
            value={config.vault_path}
            onChange={(v) => setConfig({ ...config, vault_path: v })}
            placeholder="~/Documents/Obsidian/MeinVault"
          />
        </Field>
        <p className="text-[11px] text-white/25 leading-relaxed">
          Nach dem Ändern oben rechts speichern, dann „Neu laden".
        </p>
        <button
          onClick={loadNotes}
          className="self-start px-4 py-2 rounded-lg text-[13px] font-medium border-none cursor-pointer bg-white/10 text-white/80 transition-all duration-150 hover:bg-white/[0.15]"
        >
          Neu laden
        </button>
      </FieldGroup>

      {status && (
        <div className="text-[12px] text-cyan-400/80 px-3 py-2 bg-cyan-400/[0.08] rounded-lg">
          {status}
        </div>
      )}

      <FieldGroup title={`Notizen (${notes.length})`}>
        {notes.length === 0 ? (
          <div className="text-[13px] text-white/25 text-center py-5">
            Keine Markdown-Dateien gefunden.
          </div>
        ) : (
          <div className="flex flex-col gap-1 max-h-[300px] overflow-y-auto custom-scrollbar">
            {notes.map((name) => (
              <div
                key={name}
                className="px-3 py-2 rounded-lg bg-white/[0.03] text-[13px] text-white/70"
              >
                {name}
              </div>
            ))}
          </div>
        )}
      </FieldGroup>
    </div>
  );
}
