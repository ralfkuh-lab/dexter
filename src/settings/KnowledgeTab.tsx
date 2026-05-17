import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { FieldGroup, Field, Input } from "../components/ui";

export function KnowledgeTab() {
  const [sources, setSources] = useState<[string, number][]>([]);
  const [ingesting, setIngesting] = useState(false);
  const [textSource, setTextSource] = useState("");
  const [textContent, setTextContent] = useState("");
  const [status, setStatus] = useState("");

  const loadSources = async () => {
    try {
      const result = await invoke<[string, number][]>("list_knowledge_sources");
      setSources(result);
    } catch (e) {
      console.error(e);
    }
  };

  useEffect(() => { loadSources(); }, []);

  const ingestText = async () => {
    if (!textSource.trim() || !textContent.trim()) return;
    setIngesting(true);
    setStatus("");
    try {
      const chunks = await invoke<number>("ingest_text", { source: textSource, text: textContent });
      setStatus(`Ingested ${chunks} chunks from "${textSource}"`);
      setTextSource("");
      setTextContent("");
      loadSources();
    } catch (e) {
      setStatus(`Error: ${e}`);
    }
    setIngesting(false);
  };

  const ingestFile = async () => {
    setIngesting(true);
    setStatus("");
    try {
      const path = prompt("Enter file path:");
      if (!path) { setIngesting(false); return; }
      const chunks = await invoke<number>("ingest_file", { path });
      setStatus(`Ingested ${chunks} chunks`);
      loadSources();
    } catch (e) {
      setStatus(`Error: ${e}`);
    }
    setIngesting(false);
  };

  const deleteSource = async (source: string) => {
    try {
      await invoke("delete_knowledge_source", { source });
      loadSources();
    } catch (e) {
      setStatus(`Error: ${e}`);
    }
  };

  return (
    <div className="flex flex-col gap-5 p-5 px-6">
      <p className="text-[13px] text-white/40 leading-relaxed">
        Add documents for the assistant to reference during conversations.
      </p>

      <FieldGroup title="Add Text">
        <Field label="Source Name">
          <Input value={textSource} onChange={setTextSource} placeholder="e.g. project-notes" />
        </Field>
        <Field label="Content">
          <textarea
            value={textContent}
            onChange={(e) => setTextContent(e.target.value)}
            rows={5}
            placeholder="Paste text content here..."
            className="w-full bg-white/[0.05] border border-white/10 text-white/90 px-3 py-2.5 rounded-lg text-[13px] font-inherit outline-none resize-y min-h-[80px] transition-all duration-200 focus:border-blue-500/50 focus:bg-white/[0.07] placeholder:text-white/20"
          />
        </Field>
        <div className="flex gap-2">
          <button
            onClick={ingestText}
            disabled={ingesting || !textSource.trim() || !textContent.trim()}
            className="px-4 py-2 rounded-lg text-[13px] font-medium border-none cursor-pointer bg-blue-500 text-white transition-all duration-150 hover:bg-blue-600 disabled:opacity-40 disabled:cursor-not-allowed"
          >
            {ingesting ? "Ingesting..." : "Add Text"}
          </button>
          <button
            onClick={ingestFile}
            disabled={ingesting}
            className="px-4 py-2 rounded-lg text-[13px] font-medium border-none cursor-pointer bg-white/10 text-white/80 transition-all duration-150 hover:bg-white/[0.15] disabled:opacity-40 disabled:cursor-not-allowed"
          >
            Add File
          </button>
        </div>
      </FieldGroup>

      {status && (
        <div className="text-[12px] text-cyan-400/80 px-3 py-2 bg-cyan-400/[0.08] rounded-lg">
          {status}
        </div>
      )}

      <FieldGroup title={`Sources (${sources.length})`}>
        {sources.length === 0 ? (
          <div className="text-[13px] text-white/25 text-center py-5">
            No documents in knowledge base yet.
          </div>
        ) : (
          <div className="flex flex-col gap-1">
            {sources.map(([name, chunks]) => (
              <div key={name} className="flex items-center justify-between px-3 py-2.5 rounded-lg bg-white/[0.03] hover:bg-white/[0.06] transition-colors duration-150">
                <div className="flex items-center gap-2.5">
                  <span className="text-[13px] text-white/80 font-medium">{name}</span>
                  <span className="text-[10px] text-white/25 bg-white/[0.05] px-2 py-0.5 rounded">{chunks} chunks</span>
                </div>
                <button
                  onClick={() => deleteSource(name)}
                  className="w-7 h-7 rounded-md border-none bg-red-500/10 text-red-400/60 text-base cursor-pointer flex items-center justify-center transition-all duration-150 hover:bg-red-500/20 hover:text-red-400/90"
                  title="Remove source"
                >
                  x
                </button>
              </div>
            ))}
          </div>
        )}
      </FieldGroup>
    </div>
  );
}
