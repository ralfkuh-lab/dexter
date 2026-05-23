const MODE_CONFIG: Record<string, { label: string; color: string }> = {
  chat: { label: "Chat", color: "bg-blue-500/25 text-blue-300 border-blue-500/30" },
  codex_session: { label: "Codex", color: "bg-orange-500/25 text-orange-300 border-orange-500/30" },
  claude_session: { label: "Claude", color: "bg-purple-500/25 text-purple-300 border-purple-500/30" },
  opencode_session: { label: "opencode", color: "bg-emerald-500/25 text-emerald-300 border-emerald-500/30" },
  agy_session: { label: "agy", color: "bg-cyan-500/25 text-cyan-300 border-cyan-500/30" },
};

export function ModeBar({ mode }: { mode: string }) {
  if (mode === "chat") return null;
  const cfg = MODE_CONFIG[mode] ?? { label: mode, color: "bg-white/10 text-white/60 border-white/20" };
  return (
    <div className="shrink-0 flex justify-center px-3 pt-1 pb-0.5">
      <span className={`inline-flex items-center gap-1.5 px-2.5 py-0.5 rounded-full border text-[11px] font-medium ${cfg.color}`}>
        <span className="w-1.5 h-1.5 rounded-full bg-current opacity-70" />
        {cfg.label}
      </span>
    </div>
  );
}
