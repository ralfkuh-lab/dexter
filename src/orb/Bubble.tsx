import { ChatBubble, TOOL_LABEL_MAP } from "../types";

export function Bubble({ bubble }: { bubble: ChatBubble }) {
  if (bubble.role === "user") {
    return (
      <div className="self-end max-w-[85%] animate-bubble-in">
        <div className="px-3.5 py-2.5 rounded-2xl rounded-br-md bg-[#1c2044] text-white/90 text-[13px] leading-relaxed">
          {bubble.text}
        </div>
      </div>
    );
  }

  if (bubble.role === "assistant") {
    return (
      <div className="self-start max-w-[85%] animate-bubble-in">
        <div className="px-3.5 py-2.5 rounded-2xl rounded-bl-md bg-[#252529] text-white/85 text-[13px] leading-relaxed">
          {bubble.text}
        </div>
      </div>
    );
  }

  if (bubble.role === "tool") {
    const label = TOOL_LABEL_MAP[bubble.text] || bubble.text;
    return (
      <div className="self-center animate-fade-in">
        <div className="inline-flex items-center gap-1.5 px-3 py-1 rounded-md bg-[#222228] text-white/40 text-[11px] font-medium">
          <span className="text-[10px] animate-gear-spin">&#9881;</span>
          Tool call: {label}
        </div>
      </div>
    );
  }

  if (bubble.role === "debug") {
    return (
      <div className="self-stretch animate-fade-in">
        <div className="px-3 py-2 rounded-md bg-[#101016] border border-white/[0.06] text-cyan-200/70 text-[10px] leading-relaxed font-mono break-words">
          {bubble.text}
        </div>
      </div>
    );
  }

  // status
  return (
    <div className="self-center animate-fade-in">
      <div className="px-3 py-1 text-white/25 text-[11px] font-medium">
        {bubble.text}
      </div>
    </div>
  );
}
