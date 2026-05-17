import { LlmStats } from "../types";

function formatTokens(n: number): string {
  if (n >= 1000) return `${(n / 1000).toFixed(1)}k`;
  return String(n);
}

export function StatsBar({
  model,
  ctxMax,
  stats,
}: {
  model: string;
  ctxMax: number | null;
  stats: LlmStats | null;
}) {
  const parts: string[] = [];
  const shownModel = stats?.model || model;
  if (shownModel) parts.push(shownModel);

  const effCtxMax = stats?.ctx_max ?? ctxMax;
  const prompt = stats?.prompt_tokens ?? null;
  if (prompt != null && effCtxMax != null) {
    const pct = Math.round((prompt / effCtxMax) * 100);
    parts.push(`ctx ${formatTokens(prompt)}/${formatTokens(effCtxMax)} (${pct}%)`);
  } else if (prompt != null) {
    parts.push(`ctx ${formatTokens(prompt)}`);
  } else if (effCtxMax != null) {
    parts.push(`ctx —/${formatTokens(effCtxMax)}`);
  }

  if (stats?.ttft_ms != null) parts.push(`TTFT ${stats.ttft_ms}ms`);
  if (stats?.tokens_per_sec != null) parts.push(`${stats.tokens_per_sec.toFixed(1)} tok/s`);
  if (parts.length === 0) return null;
  return (
    <div className="self-center text-[10px] text-white/35 font-mono pb-1 select-none">
      {parts.join(" · ")}
    </div>
  );
}
