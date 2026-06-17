import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { AgentDraftInfo } from "../types";

const emptyDraft: AgentDraftInfo = {
  mode: "",
  content: "",
  spoken_log: [],
  last_segment: "",
  status: "empty",
};

function modeLabel(mode: string) {
  return mode.replace("_session", "").replace(/^./, (c) => c.toUpperCase());
}

export function AgentDraft() {
  const [draft, setDraft] = useState<AgentDraftInfo>(emptyDraft);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    invoke<AgentDraftInfo>("get_agent_draft_state")
      .then(setDraft)
      .catch(() => {});

    const unlisten = listen<AgentDraftInfo>("agent_draft_updated", (event) => {
      setDraft(event.payload);
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    const el = textareaRef.current;
    if (!el) return;
    el.scrollTop = el.scrollHeight;
  }, [draft.content]);

  const update = (content: string) => {
    setDraft((prev) => ({ ...prev, content }));
    invoke("update_agent_draft", { text: content }).catch(() => {});
  };

  const submit = () => {
    invoke("submit_agent_draft").catch(() => {});
  };

  const clear = () => {
    invoke("clear_agent_draft").catch(() => {});
  };

  return (
    <div className="h-screen min-h-[560px] agent-draft-bg text-white flex flex-col overflow-hidden">
      <div className="shrink-0 px-8 pt-7 pb-4 border-b border-white/10">
        <div className="flex items-center justify-between gap-4">
          <div>
            <div className="text-[13px] uppercase tracking-[0.18em] text-sky-200/55">
              Agent Prompt
            </div>
            <h1 className="mt-1 text-[28px] leading-8 font-semibold text-white/[0.92]">
              {modeLabel(draft.mode || "agent")}
            </h1>
          </div>
          <div className="text-right">
            <div className="text-[12px] uppercase tracking-[0.16em] text-white/35">
              Status
            </div>
            <div className="mt-1 text-[18px] leading-6 text-sky-100/80">
              {draft.status || "ready"}
            </div>
          </div>
        </div>
      </div>

      <div className="flex-1 min-h-0 overflow-hidden px-8 py-7 flex flex-col gap-5">
        <textarea
          ref={textareaRef}
          value={draft.content}
          onChange={(e) => update(e.target.value)}
          placeholder="Dexter formuliert hier deinen Prompt für den Coding Agent. Sprich frei, korrigiere dich, ergänze Details. Erst klare Zustimmung sendet den Prompt."
          className="w-full flex-1 min-h-[330px] resize-none overflow-y-auto rounded-lg border border-sky-300/[0.18] bg-black/[0.42] px-6 py-5 text-[28px] leading-[1.45] text-white/[0.92] placeholder-white/[0.28] outline-none focus:border-sky-300/[0.42]"
        />

        <div className="shrink-0 grid grid-cols-[1fr_1.3fr] gap-4">
          <div className="rounded-lg border border-white/[0.08] bg-white/[0.045] px-5 py-4">
            <div className="text-[12px] uppercase tracking-[0.16em] text-white/35">
              Letzte Eingabe
            </div>
            <div className="mt-2 text-[20px] leading-8 text-white/72">
              {draft.last_segment || "Noch nichts erkannt."}
            </div>
          </div>
          <div className="rounded-lg border border-white/[0.08] bg-white/[0.045] px-5 py-4">
            <div className="text-[12px] uppercase tracking-[0.16em] text-white/35">
              Verlauf
            </div>
            <div className="mt-2 max-h-[92px] overflow-y-auto text-[16px] leading-6 text-white/58">
              {(draft.spoken_log || []).length
                ? draft.spoken_log.slice(-4).map((entry, index) => (
                    <div key={`${index}-${entry}`} className="truncate">
                      {entry}
                    </div>
                  ))
                : "Noch keine Spracheingabe."}
            </div>
          </div>
        </div>
      </div>

      <div className="shrink-0 px-8 py-5 border-t border-white/10 flex items-center justify-between gap-4">
        <div className="text-[16px] leading-6 text-white/42">
          Sag Korrekturen frei dazu. Gesendet wird erst bei klarer Zustimmung.
        </div>
        <div className="flex gap-3">
          <button
            type="button"
            onClick={clear}
            className="px-4 py-2 rounded-md border border-white/10 text-[16px] text-white/65 hover:bg-white/[0.08]"
          >
            Leeren
          </button>
          <button
            type="button"
            onClick={submit}
            disabled={!draft.content.trim()}
            className="px-5 py-2 rounded-md bg-sky-400/[0.18] text-[16px] font-medium text-sky-100 hover:bg-sky-400/[0.28] disabled:opacity-35 disabled:cursor-not-allowed"
          >
            Senden
          </button>
        </div>
      </div>
    </div>
  );
}
