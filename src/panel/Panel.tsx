import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

interface PanelContent {
  title: string;
  content: string;
}

export function Panel() {
  const [panel, setPanel] = useState<PanelContent>({ title: "", content: "" });

  useEffect(() => {
    invoke<PanelContent | null>("get_panel_content")
      .then((content) => {
        if (content) setPanel(content);
      })
      .catch(() => {});

    const unlisten = listen<PanelContent>("panel_content", (event) => {
      setPanel(event.payload);
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  return (
    <div className="panel-shell h-screen flex flex-col text-white/90 overflow-hidden">
      <div className="shrink-0 px-5 py-3 border-b border-white/10 bg-black/20">
        <h1 className="text-[14px] leading-5 font-semibold text-white/85 truncate">
          {panel.title || "Detail Panel"}
        </h1>
      </div>
      <div className="panel-markdown custom-scrollbar flex-1 overflow-y-auto px-5 py-4 text-[13px] leading-6">
        {panel.content ? (
          <ReactMarkdown remarkPlugins={[remarkGfm]}>{panel.content}</ReactMarkdown>
        ) : (
          <p className="text-white/35">Noch kein Inhalt.</p>
        )}
      </div>
    </div>
  );
}
