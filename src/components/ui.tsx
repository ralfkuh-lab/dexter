import React from "react";

export function FieldGroup({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-col gap-3 bg-white/[0.025] border border-white/[0.06] rounded-xl p-4">
      <div className="text-[11px] font-semibold text-white/40 uppercase tracking-wider">{title}</div>
      {children}
    </div>
  );
}

export function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-col gap-1.5">
      <label className="text-[12px] font-medium text-white/40">{label}</label>
      {children}
    </div>
  );
}

export function Input({
  value,
  onChange,
  placeholder,
}: {
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
}) {
  return (
    <input
      value={value}
      onChange={(e) => onChange(e.target.value)}
      placeholder={placeholder}
      className="w-full bg-white/[0.05] border border-white/10 text-white/90 px-3 py-2.5 rounded-lg text-[13px] outline-none transition-all duration-200 focus:border-blue-500/50 focus:bg-white/[0.07] placeholder:text-white/20"
    />
  );
}

export function Toggle({ on, onToggle }: { on: boolean; onToggle: () => void }) {
  return (
    <button
      onClick={onToggle}
      className={`relative w-11 h-6 rounded-full border-none cursor-pointer shrink-0 transition-colors duration-200 ${
        on ? "bg-blue-500" : "bg-white/10"
      }`}
    >
      <div
        className={`absolute top-[3px] left-[3px] w-[18px] h-[18px] rounded-full bg-white transition-transform duration-200 ${
          on ? "translate-x-5" : ""
        }`}
      />
    </button>
  );
}
