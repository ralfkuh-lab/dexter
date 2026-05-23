import { invoke } from "@tauri-apps/api/core";
import { useEffect } from "react";

export function useAutomationConsoleLogging() {
  useEffect(() => {
    const report = (message: string, source?: string, stack?: string) => {
      invoke("automation_console_error", { message, source, stack }).catch(() => {});
    };

    const onError = (event: ErrorEvent) => {
      report(event.message || "Uncaught error", event.filename, event.error?.stack);
    };

    const onUnhandledRejection = (event: PromiseRejectionEvent) => {
      const reason = event.reason;
      if (reason instanceof Error) {
        report(reason.message, "unhandledrejection", reason.stack);
      } else {
        report(String(reason), "unhandledrejection");
      }
    };

    const originalError = console.error;
    console.error = (...args: unknown[]) => {
      originalError(...args);
      report(args.map(String).join(" "), "console.error");
    };

    window.addEventListener("error", onError);
    window.addEventListener("unhandledrejection", onUnhandledRejection);

    return () => {
      window.removeEventListener("error", onError);
      window.removeEventListener("unhandledrejection", onUnhandledRejection);
      console.error = originalError;
    };
  }, []);
}
