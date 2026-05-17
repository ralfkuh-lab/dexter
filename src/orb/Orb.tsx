import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { AudioChunk, ChatBubble, LlmStats, ProcessingState, VoiceConfig } from "../types";
import { StatsBar } from "./StatsBar";
import { Bubble } from "./Bubble";

let bubbleId = 0;

export function Orb() {
  const [stage, setStage] = useState("idle");
  const [bubbles, setBubbles] = useState<ChatBubble[]>([]);
  const [mouseRecording, setMouseRecording] = useState(false);
  const [hotkey, setHotkey] = useState<string>("F9");
  const [showStats, setShowStats] = useState<boolean>(true);
  const [model, setModel] = useState<string>("");
  const [ctxMax, setCtxMax] = useState<number | null>(null);
  const [stats, setStats] = useState<LlmStats | null>(null);
  const bubblesEndRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const load = () => {
      invoke<VoiceConfig>("get_config")
        .then((c) => {
          setHotkey(c.hotkey || "F9");
          setShowStats(c.show_stats !== false);
          setModel(c.llm_model || "");
        })
        .catch(() => {});
      invoke<number | null>("get_ctx_max")
        .then((n) => setCtxMax(n))
        .catch(() => {});
      invoke<LlmStats | null>("get_last_stats")
        .then((s) => { if (s) setStats(s); })
        .catch(() => {});
    };
    load();
    const unCfg = listen("config_changed", load);
    const unStats = listen<LlmStats>("llm_stats", (e) => setStats(e.payload));
    return () => {
      unCfg.then((fn) => fn());
      unStats.then((fn) => fn());
    };
  }, []);

  const audioQueueRef = useRef<{ index: number; url: string }[]>([]);
  const isPlayingRef = useRef(false);
  const totalChunksRef = useRef<number | null>(null);
  const playedCountRef = useRef(0);
  const currentAudioRef = useRef<HTMLAudioElement | null>(null);

  useEffect(() => {
    bubblesEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [bubbles]);

  const addBubble = (role: ChatBubble["role"], text: string) => {
    setBubbles((prev) => [...prev, { role, text, id: bubbleId++ }]);
  };

  const beginManualRecording = async () => {
    if (mouseRecording) return;
    stopAllAudio();
    setMouseRecording(true);
    setStage("listening");
    try {
      await invoke("start_recording");
    } catch (e) {
      setMouseRecording(false);
      setStage("error");
      addBubble("status", String(e));
    }
  };

  const endManualRecording = async () => {
    if (!mouseRecording) return;
    setMouseRecording(false);
    setStage("transcribing");
    try {
      await invoke("stop_recording_and_process");
    } catch (e) {
      setStage("error");
      addBubble("status", String(e));
    }
  };

  const stopAllAudio = () => {
    if (currentAudioRef.current) {
      currentAudioRef.current.pause();
      currentAudioRef.current.onended = null;
      currentAudioRef.current.onerror = null;
      currentAudioRef.current = null;
    }
    for (const item of audioQueueRef.current) {
      URL.revokeObjectURL(item.url);
    }
    audioQueueRef.current = [];
    isPlayingRef.current = false;
    totalChunksRef.current = null;
    playedCountRef.current = 0;
  };

  const playNext = () => {
    if (isPlayingRef.current) return;
    audioQueueRef.current.sort((a, b) => a.index - b.index);
    if (audioQueueRef.current.length === 0) {
      if (totalChunksRef.current !== null && playedCountRef.current >= totalChunksRef.current) {
        setStage("idle");
        totalChunksRef.current = null;
        playedCountRef.current = 0;
      }
      return;
    }
    const next = audioQueueRef.current.shift()!;
    isPlayingRef.current = true;
    const audio = new Audio(next.url);
    currentAudioRef.current = audio;
    audio.play().catch(() => {});
    audio.onended = () => { URL.revokeObjectURL(next.url); currentAudioRef.current = null; isPlayingRef.current = false; playedCountRef.current++; playNext(); };
    audio.onerror = () => { URL.revokeObjectURL(next.url); currentAudioRef.current = null; isPlayingRef.current = false; playedCountRef.current++; playNext(); };
  };

  useEffect(() => {
    const unInterrupted = listen("pipeline_interrupted", () => {
      stopAllAudio();
    });
    const unPressed = listen("hotkey_pressed", () => {
      stopAllAudio();
      setStage("listening");
    });
    const unReleased = listen("hotkey_released", () => { setStage("transcribing"); });
    return () => {
      unInterrupted.then((fn) => fn());
      unPressed.then((fn) => fn());
      unReleased.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    const unlisten = listen<ProcessingState>("processing", (event) => {
      const { stage: newStage, text } = event.payload;
      setStage(newStage);
      if (newStage === "transcribed") {
        addBubble("user", text);
      } else if (newStage === "tool_call") {
        addBubble("tool", text);
      } else if (newStage === "speaking") {
        // Remove ephemeral status chips when assistant starts speaking.
        setBubbles((prev) => {
          const filtered = prev.filter((b) => b.role !== "status");
          // Walk backwards: skip debug bubbles (LLM request/response chips that
          // get emitted between streamed sentences) and merge into the most
          // recent assistant bubble of this turn. Stop at user/tool boundaries
          // so a new turn always starts a fresh bubble.
          for (let i = filtered.length - 1; i >= 0; i--) {
            const b = filtered[i];
            if (b.role === "debug") continue;
            if (b.role === "assistant") {
              const updated = [...filtered];
              updated[i] = { ...b, text };
              return updated;
            }
            break;
          }
          return [...filtered, { role: "assistant", text, id: bubbleId++ }];
        });
      } else if (newStage === "error") {
        addBubble("status", text);
      }
    });
    return () => { unlisten.then((fn) => fn()); };
  }, []);

  useEffect(() => {
    const unlisten = listen<string>("llm_debug", (event) => {
      addBubble("debug", event.payload);
    });
    return () => { unlisten.then((fn) => fn()); };
  }, []);

  useEffect(() => {
    const unlisten = listen<AudioChunk>("play_audio_chunk", (event) => {
      const { index, audio } = event.payload;
      const audioBytes = Uint8Array.from(atob(audio), (c) => c.charCodeAt(0));
      const audioBlob = new Blob([audioBytes], { type: "audio/wav" });
      const url = URL.createObjectURL(audioBlob);
      audioQueueRef.current.push({ index, url });
      playNext();
    });
    return () => { unlisten.then((fn) => fn()); };
  }, []);

  useEffect(() => {
    const unlisten = listen<number>("play_audio_done", (event) => {
      totalChunksRef.current = event.payload;
      if (playedCountRef.current >= event.payload && !isPlayingRef.current) {
        setStage("idle");
      }
    });
    return () => { unlisten.then((fn) => fn()); };
  }, []);

  useEffect(() => {
    const unlisten = listen("messages_cleared", () => { setBubbles([]); setStage("idle"); });
    return () => { unlisten.then((fn) => fn()); };
  }, []);

  const orbClass = [
    "orb-container",
    stage === "listening" && "orb-listening",
    stage === "transcribing" && "orb-processing",
    stage === "transcribed" && "orb-processing",
    stage === "thinking" && "orb-thinking",
    stage === "tool_call" && "orb-toolcall",
    stage === "speaking" && "orb-speaking",
    stage === "error" && "orb-error",
  ].filter(Boolean).join(" ");

  const glowAnim =
    stage === "listening" ? "animate-pulse-slow" :
    stage === "speaking" ? "animate-speak-pulse" :
    stage === "tool_call" ? "animate-breathe-fast" :
    (stage === "processing" || stage === "transcribing" || stage === "transcribed") ? "animate-breathe-fast" :
    stage === "thinking" ? "animate-breathe" :
    stage === "error" ? "" :
    "animate-breathe";

  const ringAnim =
    stage === "listening" ? "animate-ring-pulse" :
    (stage === "transcribing" || stage === "transcribed" || stage === "processing") ? "animate-spin-medium" :
    stage === "thinking" ? "animate-spin-slow" :
    stage === "tool_call" ? "animate-spin-fast" :
    "";

  return (
    <div className="flex flex-col h-screen orb-bg px-5 py-4">
      {/* Conversation bubbles */}
      <div className="flex-1 overflow-y-auto flex flex-col justify-end px-3.5 pt-4 pb-2.5 gap-2 no-scrollbar bubble-mask">
        {bubbles.map((b) => (
          <Bubble key={b.id} bubble={b} />
        ))}
        {(stage === "listening" || stage === "transcribing" || stage === "thinking") && (
          <div className="self-center animate-fade-in px-3 py-1 text-white/25 text-[11px] font-medium">
            {stage === "listening" ? "Listening..." : stage === "transcribing" ? "Transcribing..." : "Thinking..."}
          </div>
        )}
        <div ref={bubblesEndRef} />
      </div>

      {showStats && <StatsBar model={model} ctxMax={ctxMax} stats={stats} />}

      {/* Orb */}
      <div className="flex justify-center pb-5 pt-2 shrink-0">
        <div
          className={`${orbClass} relative w-20 h-20 cursor-pointer select-none`}
          title={`Push to talk — hold orb or ${hotkey}`}
          onPointerDown={(e) => {
            e.currentTarget.setPointerCapture(e.pointerId);
            beginManualRecording();
          }}
          onPointerUp={(e) => {
            e.currentTarget.releasePointerCapture(e.pointerId);
            endManualRecording();
          }}
          onPointerCancel={endManualRecording}
          onPointerLeave={() => {
            if (mouseRecording) endManualRecording();
          }}
        >
          <div className={`orb-glow absolute -inset-[5%] rounded-full blur-[14px] z-[1] ${glowAnim}`} />
          <div className="orb-core absolute inset-[18%] rounded-full z-[2]" />
          <div className={`orb-ring absolute inset-[8%] rounded-full border-[1.5px] z-[3] ${ringAnim}`} />
        </div>
      </div>
    </div>
  );
}
