import { useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";

const COMMANDS = [
  { cmd: "lösche Wort", desc: "letztes Wort" },
  { cmd: "lösche Satz", desc: "letzten Satz" },
  { cmd: "lösche alles", desc: "Buffer leeren" },
  { cmd: "neue Zeile", desc: "Umbruch" },
  { cmd: "over", desc: "absenden" },
];

export function DictationBuffer({
  buffer,
  onBufferChange,
}: {
  buffer: string;
  onBufferChange: (text: string) => void;
}) {
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    const el = textareaRef.current;
    if (!el) return;
    el.style.height = "auto";
    const max = 8 * 20 + 16;
    el.style.height = `${Math.min(el.scrollHeight, max)}px`;
  }, [buffer]);

  useEffect(() => {
    setTimeout(() => textareaRef.current?.focus(), 50);
  }, []);

  const send = () => {
    invoke("send_dictation").catch(() => {});
  };

  const cancel = () => {
    invoke("toggle_dictation").catch(() => {});
  };

  return (
    <div className="shrink-0 mx-2 mb-2 rounded-lg border border-emerald-500/20 bg-emerald-950/30 animate-fade-in">
      <div className="px-3 pt-2 pb-1">
        <div className="flex items-center justify-between mb-1.5">
          <span className="text-[11px] font-medium text-emerald-300/70">Diktier-Modus</span>
          <div className="flex gap-1.5">
            <button
              type="button"
              onClick={cancel}
              className="px-2 py-0.5 rounded text-[10px] font-medium text-white/40 hover:text-white/70 hover:bg-white/5 transition-colors"
            >
              Abbrechen
            </button>
            <button
              type="button"
              onClick={send}
              disabled={!buffer.trim()}
              className="px-2.5 py-0.5 rounded text-[10px] font-medium bg-emerald-500/20 text-emerald-300 hover:bg-emerald-500/30 transition-colors disabled:opacity-30 disabled:cursor-not-allowed"
            >
              Senden
            </button>
          </div>
        </div>
        <textarea
          ref={textareaRef}
          value={buffer}
          onChange={(e) => {
            onBufferChange(e.target.value);
            invoke("update_dictation_buffer", { text: e.target.value }).catch(() => {});
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              if (buffer.trim()) send();
            } else if (e.key === "Escape") {
              cancel();
            }
          }}
          rows={1}
          placeholder="Sprich einen Satz … (F9 gedrückt halten)"
          className="block w-full px-3 py-2 text-sm rounded-md bg-black/30 text-white/90 placeholder-white/25 border border-emerald-500/10 focus:outline-none focus:border-emerald-500/30 resize-none overflow-y-auto leading-5"
        />
      </div>
      <div className="px-3 py-1.5 border-t border-emerald-500/10 bg-emerald-950/20 rounded-b-lg flex flex-wrap gap-x-3 gap-y-0.5">
        {COMMANDS.map((c) => (
          <span key={c.cmd} className="text-[10px] text-emerald-400/40">
            <span className="text-emerald-300/50">&ldquo;{c.cmd}&rdquo;</span>{" "}
            {c.desc}
          </span>
        ))}
      </div>
    </div>
  );
}
