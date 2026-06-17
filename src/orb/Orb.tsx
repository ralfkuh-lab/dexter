import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { AudioChunk, ChatBubble, DebugEvent, DialogPayload, LlmStats, ProcessingState, VoiceConfig } from "../types";
import { StatsBar } from "./StatsBar";
import { ModeBar } from "./ModeBar";
import { DictationBuffer } from "./DictationBuffer";
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
  const [ttsEnabled, setTtsEnabled] = useState<boolean>(true);
  const [textInputVisible, setTextInputVisible] = useState<boolean>(false);
  const [textInput, setTextInput] = useState<string>("");
  const [dialog, setDialog] = useState<DialogPayload | null>(null);
  const [appMode, setAppMode] = useState<string>("chat");
  const [handsFreeActive, setHandsFreeActive] = useState<boolean>(false);
  const [dictationActive, setDictationActive] = useState<boolean>(false);
  const [dictationBuffer, setDictationBuffer] = useState<string>("");
  const [dictationSpeech, setDictationSpeech] = useState<boolean>(false);
  const bubblesEndRef = useRef<HTMLDivElement>(null);
  const textInputRef = useRef<HTMLTextAreaElement>(null);

  /// Höhe an Inhalt anpassen — wächst bis ~5 Zeilen, danach scrollt das
  /// Textarea intern.
  const autoresizeTextarea = () => {
    const el = textInputRef.current;
    if (!el) return;
    el.style.height = "auto";
    const max = 5 * 20 + 16; // ~5 Zeilen + Padding
    el.style.height = `${Math.min(el.scrollHeight, max)}px`;
  };

  useEffect(() => {
    const load = () => {
      invoke<VoiceConfig>("get_config")
        .then((c) => {
          setHotkey(c.hotkey || "F9");
          setShowStats(c.show_stats !== false);
          setModel(c.llm_model || "");
          setTtsEnabled(c.tts_enabled !== false);
        })
        .catch(() => {});
      invoke<string>("get_app_mode")
        .then((m) => setAppMode(m))
        .catch(() => {});
      invoke<[boolean, string]>("get_dictation_state")
        .then(([active, buf]) => { setDictationActive(active); setDictationBuffer(buf); })
        .catch(() => {});
      invoke<boolean>("get_hands_free_state")
        .then((active) => setHandsFreeActive(active))
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
    const unMode = listen<string>("app_mode_changed", (e) => setAppMode(e.payload));
    const unHandsFree = listen<boolean>("hands_free_mode_changed", (e) => {
      setHandsFreeActive(e.payload);
      if (!e.payload) setDictationSpeech(false);
    });
    const unDictMode = listen<boolean>("dictation_mode_changed", (e) => {
      setDictationActive(e.payload);
      if (!e.payload) {
        setDictationBuffer("");
        setDictationSpeech(false);
      }
    });
    const unDictBuf = listen<string>("dictation_buffer_updated", (e) => setDictationBuffer(e.payload));
    const unDictVad = listen<{ rms: number; threshold: number; speech: boolean }>("dictation_vad", (e) => {
      setDictationSpeech(e.payload.speech);
    });
    return () => {
      unCfg.then((fn) => fn());
      unStats.then((fn) => fn());
      unMode.then((fn) => fn());
      unHandsFree.then((fn) => fn());
      unDictMode.then((fn) => fn());
      unDictBuf.then((fn) => fn());
      unDictVad.then((fn) => fn());
    };
  }, []);

  const audioQueueRef = useRef<{ index: number; url: string }[]>([]);
  const isPlayingRef = useRef(false);
  const totalChunksRef = useRef<number | null>(null);
  const playedCountRef = useRef(0);
  const currentAudioRef = useRef<HTMLAudioElement | null>(null);

  const markSpeaking = (speaking: boolean) => {
    invoke("set_speaking", { speaking }).catch(() => {});
  };

  useEffect(() => {
    bubblesEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [bubbles]);

  const addBubble = (role: ChatBubble["role"], text: string, detail?: string) => {
    setBubbles((prev) => [...prev, { role, text, id: bubbleId++, detail }]);
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
    markSpeaking(false);
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
      markSpeaking(false);
      return;
    }
    const next = audioQueueRef.current.shift()!;
    isPlayingRef.current = true;
    const audio = new Audio(next.url);
    currentAudioRef.current = audio;
    markSpeaking(true);
    audio.play().catch(() => {
      URL.revokeObjectURL(next.url);
      currentAudioRef.current = null;
      isPlayingRef.current = false;
      playedCountRef.current++;
      playNext();
    });
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
    const unlisten = listen<string>("assistant_text", (event) => {
      addBubble("assistant", event.payload);
    });
    return () => { unlisten.then((fn) => fn()); };
  }, []);

  useEffect(() => {
    const unlisten = listen<DebugEvent>("llm_debug", (event) => {
      addBubble("debug", event.payload.summary, event.payload.detail);
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

  useEffect(() => {
    const unlisten = listen("focus_text_input", () => {
      setTextInputVisible(true);
      // Defer focus until the input is in the DOM.
      setTimeout(() => textInputRef.current?.focus(), 50);
    });
    return () => { unlisten.then((fn) => fn()); };
  }, []);

  useEffect(() => {
    const unShow = listen<DialogPayload>("show_dialog", (event) => {
      setDialog(event.payload);
      setStage("idle");
    });
    const unDismiss = listen("dismiss_dialog", () => {
      setDialog(null);
    });
    return () => {
      unShow.then((fn) => fn());
      unDismiss.then((fn) => fn());
    };
  }, []);

  const toggleTts = () => {
    const next = !ttsEnabled;
    setTtsEnabled(next);
    invoke("set_tts_enabled", { enabled: next }).catch(() => {});
    if (!next) stopAllAudio();
  };

  const toggleHandsFree = () => {
    stopAllAudio();
    invoke("toggle_hands_free").catch((e) => addBubble("status", String(e)));
  };

  const toggleTextInput = () => {
    setTextInputVisible((v) => {
      const next = !v;
      if (next) setTimeout(() => textInputRef.current?.focus(), 50);
      return next;
    });
  };

  const submitText = async () => {
    const text = textInput.trim();
    if (!text) return;
    setTextInput("");
    if (textInputRef.current) textInputRef.current.style.height = "auto";
    stopAllAudio();
    setStage("thinking");
    try {
      await invoke("submit_text", { text });
    } catch (e) {
      setStage("error");
      addBubble("status", String(e));
    }
  };

  const chooseDialogOption = async (label: string) => {
    try {
      await invoke("resolve_dialog", { selected: label });
      setDialog(null);
    } catch (e) {
      addBubble("status", String(e));
    }
  };

  const orbClass = [
    "orb-container",
    handsFreeActive && "orb-hands-free",
    handsFreeActive && dictationSpeech && "orb-hands-free-speaking",
    dictationActive && "orb-dictation",
    dictationActive && dictationSpeech && "orb-dictation-speaking",
    stage === "listening" && "orb-listening",
    stage === "transcribing" && "orb-processing",
    stage === "transcribed" && "orb-processing",
    stage === "thinking" && "orb-thinking",
    stage === "tool_call" && "orb-toolcall",
    stage === "speaking" && "orb-speaking",
    stage === "error" && "orb-error",
  ].filter(Boolean).join(" ");

  const glowAnim =
    handsFreeActive ? (dictationSpeech ? "animate-pulse-slow" : "animate-breathe") :
    dictationActive ? (dictationSpeech ? "animate-pulse-slow" : "animate-breathe") :
    stage === "listening" ? "animate-pulse-slow" :
    stage === "speaking" ? "animate-speak-pulse" :
    stage === "tool_call" ? "animate-breathe-fast" :
    (stage === "processing" || stage === "transcribing" || stage === "transcribed") ? "animate-breathe-fast" :
    stage === "thinking" ? "animate-breathe" :
    stage === "error" ? "" :
    "animate-breathe";

  const ringAnim =
    handsFreeActive ? (dictationSpeech ? "animate-ring-pulse" : "") :
    dictationActive ? (dictationSpeech ? "animate-ring-pulse" : "") :
    stage === "listening" ? "animate-ring-pulse" :
    (stage === "transcribing" || stage === "transcribed" || stage === "processing") ? "animate-spin-medium" :
    stage === "thinking" ? "animate-spin-slow" :
    stage === "tool_call" ? "animate-spin-fast" :
    "";

  return (
    <div className="flex flex-col h-screen orb-bg px-5 py-4">
      <ModeBar mode={appMode} />
      {/* Conversation bubbles */}
      <div className="flex-1 overflow-y-auto flex flex-col justify-end px-3.5 pt-4 pb-2.5 gap-2 no-scrollbar bubble-mask">
        {bubbles.map((b) => (
          <Bubble key={b.id} bubble={b} />
        ))}
        {(stage === "listening" || stage === "transcribing" || stage === "thinking") && (
          <div className="self-center animate-fade-in px-3 py-1 text-white/25 text-[11px] font-medium">
            {stage === "listening" ? (handsFreeActive ? "Hands-free listening..." : "Listening...") : stage === "transcribing" ? "Transcribing..." : "Thinking..."}
          </div>
        )}
        <div ref={bubblesEndRef} />
      </div>

      {dialog && (
        <div className="shrink-0 mx-2 mb-2 rounded-lg border border-white/10 bg-black/35 glass-subtle p-3 animate-fade-in">
          <div className="text-[12px] leading-5 font-medium text-white/85 mb-2">
            {dialog.question}
          </div>
          <div className="flex flex-col gap-1.5">
            {dialog.options.map((option, index) => (
              <button
                key={`${option.label}-${index}`}
                type="button"
                onClick={() => chooseDialogOption(option.label)}
                className="w-full flex items-start gap-2 rounded-md border border-white/10 bg-white/[0.04] hover:bg-white/[0.08] px-2.5 py-2 text-left transition-colors"
              >
                <span className="shrink-0 w-5 h-5 rounded bg-cyan-400/15 text-cyan-200 text-[11px] leading-5 text-center font-semibold">
                  {String.fromCharCode(65 + index)}
                </span>
                <span className="min-w-0 flex-1">
                  <span className="block text-[12px] leading-4 text-white/85">{option.label}</span>
                  {option.description && (
                    <span className="block text-[11px] leading-4 text-white/45 mt-0.5">
                      {option.description}
                    </span>
                  )}
                </span>
              </button>
            ))}
          </div>
        </div>
      )}

      {dictationActive && (
        <DictationBuffer
          buffer={dictationBuffer}
          listening={dictationSpeech}
          onBufferChange={setDictationBuffer}
        />
      )}

      {textInputVisible && !dictationActive && (
        <div className="shrink-0 px-2 pt-1 pb-1">
          <textarea
            ref={textInputRef}
            value={textInput}
            onChange={(e) => {
              setTextInput(e.target.value);
              autoresizeTextarea();
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                submitText();
              } else if (e.key === "Escape") {
                setTextInputVisible(false);
              }
            }}
            rows={1}
            placeholder="Nachricht … (Enter senden, Shift+Enter Zeilenumbruch)"
            className="block w-full px-3 py-2 text-sm rounded-md bg-white/5 text-white placeholder-white/30 border border-white/10 focus:outline-none focus:border-white/30 focus:bg-white/10 resize-none overflow-y-auto leading-5"
          />
        </div>
      )}

      {showStats && <StatsBar model={model} ctxMax={ctxMax} stats={stats} />}

      {/* Toolbar mit Lautsprecher- und Tastatur-Toggle */}
      <div className="flex justify-end gap-1.5 px-1 pt-1 shrink-0">
        <button
          type="button"
          onClick={toggleHandsFree}
          className={`p-1.5 rounded-md transition-colors ${handsFreeActive ? "text-sky-200 bg-sky-400/10 hover:bg-sky-400/15" : "text-white/50 hover:text-white/80 hover:bg-white/5"}`}
          title={handsFreeActive ? "Hands-free aktiv (klick zum Stoppen)" : "Hands-free einschalten"}
        >
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <path d="M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z" />
            <path d="M19 10v2a7 7 0 0 1-14 0v-2" />
            <path d="M12 19v3" />
          </svg>
        </button>
        <button
          type="button"
          onClick={toggleTts}
          className={`p-1.5 rounded-md transition-colors ${ttsEnabled ? "text-white/70 hover:text-white/90 hover:bg-white/5" : "text-white/30 hover:text-white/50 hover:bg-white/5"}`}
          title={ttsEnabled ? "Sprachausgabe an (klick zum Stummschalten)" : "Sprachausgabe aus (klick zum Aktivieren)"}
        >
          {ttsEnabled ? (
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <path d="M11 5L6 9H2v6h4l5 4z" />
              <path d="M15.54 8.46a5 5 0 0 1 0 7.07" />
              <path d="M19.07 4.93a10 10 0 0 1 0 14.14" />
            </svg>
          ) : (
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <path d="M11 5L6 9H2v6h4l5 4z" />
              <line x1="22" y1="9" x2="16" y2="15" />
              <line x1="16" y1="9" x2="22" y2="15" />
            </svg>
          )}
        </button>
        <button
          type="button"
          onClick={toggleTextInput}
          className={`p-1.5 rounded-md transition-colors ${textInputVisible ? "text-white/90 bg-white/5 hover:bg-white/10" : "text-white/50 hover:text-white/80 hover:bg-white/5"}`}
          title="Texteingabe ein-/ausblenden"
        >
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <rect x="2" y="6" width="20" height="12" rx="2" />
            <path d="M6 10h.01M10 10h.01M14 10h.01M18 10h.01M7 14h10" />
          </svg>
        </button>
      </div>

      {/* Orb */}
      <div className="flex justify-center pb-2 pt-1 shrink-0">
        <div
          className={`${orbClass} relative w-20 h-20 cursor-pointer select-none`}
          title={
            handsFreeActive
              ? "Hands-free aktiv — klick zum Stoppen"
              : dictationActive
                ? "Diktier-Modus aktiv — klick zum Stoppen"
                : `Push to talk — hold orb or ${hotkey}`
          }
          onPointerDown={(e) => {
            if (handsFreeActive) {
              toggleHandsFree();
              return;
            }
            if (dictationActive) {
              invoke("toggle_dictation").catch(() => {});
              return;
            }
            e.currentTarget.setPointerCapture(e.pointerId);
            beginManualRecording();
          }}
          onPointerUp={(e) => {
            if (handsFreeActive || dictationActive) return;
            e.currentTarget.releasePointerCapture(e.pointerId);
            endManualRecording();
          }}
          onPointerCancel={() => {
            if (!dictationActive && !handsFreeActive) endManualRecording();
          }}
          onPointerLeave={() => {
            if (!dictationActive && !handsFreeActive && mouseRecording) endManualRecording();
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
