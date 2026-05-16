# Multi-OS Migration Plan

## Goal

Make Dexter usable on macOS, Linux, and Windows without forking the application logic per operating system.

The app should keep one shared core for voice capture, transcription, Ollama, Chatterbox TTS, RAG, streaming, and UI state. OS-specific code should be isolated behind small platform adapters for desktop integration: screenshots, clipboard, URL opening, app/window listing, shell execution, window behavior, tray, hotkeys, packaging, and install prerequisites.

## Answer to the Linux Plan Question

The Linux plan does not intentionally make the app Linux-only. It preserves the existing macOS behavior by moving macOS-specific logic behind platform modules and adding Linux implementations next to it.

However, that plan is Linux-focused. It does not fully specify Windows behavior, packaging, testing, or platform APIs. This document extends the migration into a true macOS/Linux/Windows strategy.

## Current Architecture Assessment

Likely portable with little or no OS-specific work:

- React/Vite frontend.
- Tauri command/event bridge.
- Ollama HTTP client.
- Chatterbox HTTP client.
- SQLite RAG store with bundled SQLite.
- Whisper model path concept.
- Streaming sentence-to-TTS pipeline.
- `cpal` audio capture in principle, because it supports CoreAudio, ALSA/Pulse/PipeWire through ALSA, and WASAPI.

Needs platform abstraction:

- Screenshot capture and image resizing.
- Clipboard access.
- Open URL in default browser.
- List visible apps/windows/processes.
- Sandbox shell and safe PATH.
- Tray behavior.
- Global push-to-talk shortcut.
- Overlay transparency, always-on-top behavior, skip-taskbar behavior, and bottom-right positioning.
- Build prerequisites and packaging.

## Target Architecture

Create a platform layer in Rust:

```text
src-tauri/src/platform/
  mod.rs
  common.rs
  macos.rs
  linux.rs
  windows.rs
```

Expose a stable API to the rest of the backend:

```rust
pub fn take_screenshot(monitor: Option<u32>) -> Result<String, String>;
pub fn read_clipboard() -> Result<String, String>;
pub fn open_url(url: &str) -> Result<String, String>;
pub fn list_visible_apps() -> Result<String, String>;
pub fn shell_program() -> &'static str;
pub fn safe_path() -> &'static str;
pub fn platform_name() -> &'static str;
```

Keep shared implementation in `common.rs`:

- JPEG encoding and resizing.
- URL validation.
- common command helpers.
- error formatting.
- `describe_screenshot`.
- `get_current_time`.
- `web_fetch`.

Use `#[cfg(target_os = "...")]` only inside the platform layer and small setup blocks in `lib.rs`. The rest of the app should call platform functions without knowing the target OS.

## Feature Parity Matrix

| Feature | macOS | Linux | Windows | Notes |
|---|---|---|---|---|
| Tauri UI | Keep | Add/test | Add/test | Tauri supports all three, but Linux uses WebKitGTK and Windows uses WebView2. |
| Tray | Keep | Test AppIndicator | Test notification area | Desktop environment behavior differs. |
| Global hotkey | Keep | Risk on Wayland | Likely viable | Push-to-talk requires press and release events. |
| Microphone capture | CoreAudio | ALSA/Pulse/PipeWire | WASAPI | `cpal` is the right abstraction, but device selection needs testing. |
| Whisper STT | Keep | Add deps/test | Add deps/test | Native build tools vary by OS. |
| Ollama | Keep | Keep | Keep | Assumes local Ollama service installed. |
| Chatterbox TTS | Keep | Keep | Keep | Assumes local compatible HTTP server. |
| Screenshot | `screencapture` or native crate | portal/grim/X11 backend | Windows capture API/native crate | Needs common resize/JPEG path. |
| Clipboard | replace `pbpaste` or keep gated | plugin/native/CLI fallback | plugin/native | Prefer Tauri clipboard plugin. |
| Open URL | `open` or opener plugin | `xdg-open` or opener plugin | opener plugin / ShellExecute | Prefer Tauri opener plugin. |
| App listing | AppleScript | X11/window/process fallback | process/window enumeration | Semantics differ by OS. |
| Sandbox shell | `zsh` or `/bin/zsh` | `/bin/sh` or `/bin/bash` | PowerShell or `cmd` | Tool prompt must mention the actual shell. |
| Packaging | `.app`/DMG | AppImage/deb/rpm | NSIS/MSI | Build on native OS runners. |

## Build and Dependency Strategy

### Shared changes

- Keep `tauri` dependency cross-platform with only common features globally.
- Move `macos-private-api` to macOS-only configuration if possible.
- Consider switching `reqwest` to `rustls-tls` to reduce native TLS friction:

```toml
reqwest = { version = "0.12", default-features = false, features = ["json", "multipart", "stream", "rustls-tls"] }
```

- Add `image` for shared screenshot resizing and JPEG encoding.
- Evaluate a cross-platform screenshot crate first. If it is reliable across all targets, use it behind the platform layer; otherwise use per-OS backends.
- Add a platform-neutral clipboard approach, preferably `tauri-plugin-clipboard-manager`, if backend access works cleanly.
- Keep OS-specific helper commands as fallbacks, not as the primary architecture.

### macOS prerequisites

- Xcode Command Line Tools.
- Rust stable.
- Node/npm.
- Ollama.
- Chatterbox server.
- Whisper GGML model.

Existing macOS helpers can continue initially:

- `screencapture`
- `sips`
- `pbpaste`
- `open`
- `osascript`

Long-term, prefer the same plugin/native abstractions used by other OSes where practical.

### Linux prerequisites

Follow Tauri 2 Linux prerequisites:

```bash
sudo apt update
sudo apt install libwebkit2gtk-4.1-dev \
  build-essential \
  curl \
  wget \
  file \
  libxdo-dev \
  libssl-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev
```

Project-specific additions:

```bash
sudo apt install pkg-config cmake clang libasound2-dev
```

Optional helper tools if CLI fallbacks are used:

```bash
sudo apt install xdg-utils wl-clipboard xclip grim slurp imagemagick wmctrl
```

Tauri prerequisite source: <https://v2.tauri.app/start/prerequisites/>

### Windows prerequisites

Follow Tauri 2 Windows prerequisites:

- Microsoft C++ Build Tools with "Desktop development with C++".
- Microsoft Edge WebView2 Runtime. Modern Windows often already has it, but installers should handle older machines.
- Rust stable MSVC toolchain.
- Node/npm.
- CMake and LLVM/Clang if required by native crates during `whisper-rs` builds.
- Ollama for Windows.
- Chatterbox server or a documented Windows-compatible TTS server setup.
- Whisper GGML model.

Tauri prerequisite source: <https://v2.tauri.app/start/prerequisites/>

## Platform Tool Plan

### Screenshots

Use one shared output contract:

- returns base64 JPEG.
- max dimension 1280px.
- quality around 70.
- clear error messages when OS permissions or backends block capture.

macOS:

- Keep `screencapture` for first pass.
- Replace `sips` resizing with shared Rust `image` resizing.
- Later evaluate native capture or cross-platform screenshot crate.

Linux:

- Wayland portal-first where possible.
- `grim` fallback for wlroots compositors.
- X11 fallback using native crate or CLI helper.
- Return a limitation message on locked-down Wayland sessions.

Windows:

- Prefer a Rust/native implementation using Windows capture APIs or a proven screenshot crate.
- Avoid invoking GUI tools for capture.
- Handle multi-monitor selection explicitly.
- Document permission prompts if any backend requires them.

### Clipboard

Preferred path for all OSes:

- Use Tauri clipboard plugin or a Rust clipboard crate behind `platform::read_clipboard`.

Fallbacks:

- macOS: `pbpaste`.
- Linux Wayland: `wl-paste`.
- Linux X11: `xclip` or `xsel`.
- Windows: native clipboard API via crate; avoid PowerShell if possible.

### Open URL

Preferred path:

- Use `tauri-plugin-opener`, already present in the project.

Fallbacks:

- macOS: `open`.
- Linux: `xdg-open`.
- Windows: ShellExecute/native opener.

Before opening:

- Validate `http`, `https`, and explicitly supported app schemes.
- Reject shell-looking input, file paths, and empty strings unless the product intentionally supports them.

### App/window listing

This is not truly equivalent across OSes. Define two separate concepts:

- visible windows: what the user likely means by "apps I have open".
- running processes: lower-level fallback.

macOS:

- Keep AppleScript first pass.

Linux:

- X11: `wmctrl -lx` or native X11 window enumeration.
- Wayland: likely unavailable globally; return limitation or process fallback.

Windows:

- Use Windows window enumeration for visible top-level windows.
- Optional process fallback via Rust crate.

Update tool descriptions so the model says "visible windows" or "running processes" accurately for the current OS.

### Sandbox Shell and PATH

Move shell and PATH to platform functions:

| OS | Shell | PATH |
|---|---|---|
| macOS | `/bin/zsh` or `/bin/sh` | `/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin` |
| Linux | `/bin/sh` or `/bin/bash` | `/usr/local/bin:/usr/bin:/bin:/usr/local/sbin:/usr/sbin:/sbin` |
| Windows | PowerShell 7 if available, else Windows PowerShell or `cmd` | System PATH with secrets stripped |

Update `voice.rs` tool text dynamically so it does not always say "runs in zsh".

Expand the sandbox blocklist per OS:

- macOS: existing `launchctl`, `diskutil`, keychain patterns.
- Linux: `systemctl poweroff`, `systemctl reboot`, `mkfs`, `mount`, `umount`, `modprobe`, destructive package manager commands.
- Windows: `Remove-Item -Recurse -Force C:\`, `Format-Volume`, `bcdedit`, `shutdown`, registry destructive commands, credential manager extraction.

## Desktop Integration Plan

### Tray

Keep Tauri tray as the default.

Testing requirements:

- macOS menu bar.
- Linux Mint/Cinnamon or target DE.
- GNOME Wayland with AppIndicator extension status documented.
- KDE Plasma.
- Windows notification area.

Fallback:

- If tray is unavailable, show a normal window with settings and quit controls.

### Global hotkeys

The current UX depends on press/release for `Shift+Z`.

Risks:

- Wayland may block or restrict global shortcuts.
- Some OSes may not deliver release events reliably for modifier combos.
- Windows may reserve or interfere with some combinations.

Plan:

- Keep current push-to-talk where supported.
- Add configurable hotkey.
- Add fallback "toggle-to-talk" mode.
- Add visible in-window microphone button as a universal fallback.
- Store hotkey mode in config.

### Overlay Window

Current behavior:

- transparent frameless window.
- always on top.
- skip taskbar.
- positioned bottom-right with a hard-coded macOS dock offset.

Plan:

- Move positioning into platform helper.
- Remove hard-coded dock offset from shared logic.
- Use screen work area when available.
- Test transparency and click/focus behavior per OS.
- If transparency is unreliable on Linux, support a decorated compact mode.

## Configuration and Data Paths

Keep `dirs::config_dir()` and document expected locations:

- macOS: `~/Library/Application Support/voice-assistant/config.json`
- Linux: `~/.config/voice-assistant/config.json`
- Windows: `%APPDATA%\voice-assistant\config.json`

Add a diagnostic command or settings panel row showing the actual config path at runtime.

Whisper model defaults can stay under home cache, but document per OS:

- macOS/Linux: `~/.cache/whisper/ggml-base.en.bin`
- Windows: `%LOCALAPPDATA%\whisper\ggml-base.en.bin` or an app-managed data dir.

## Packaging Plan

Use native CI runners. Do not rely on cross-compiling for the initial release.

Targets:

- macOS: app bundle and DMG.
- Linux: AppImage first, Debian package second if Linux Mint/Ubuntu is the target.
- Windows: NSIS installer first, MSI later if enterprise deployment matters.

CI matrix:

```text
macos-latest: npm run build, cargo check, cargo tauri build
ubuntu-latest: npm run build, cargo check, cargo tauri build
windows-latest: npm run build, cargo check, cargo tauri build
```

Smoke tests should run before packaging:

- app starts.
- settings window opens.
- config loads/saves.
- tray menu appears or fallback window appears.
- microphone device can be enumerated.
- local Ollama URL is configurable.
- disabled external services produce clean errors.

## Implementation Phases

### Phase 1: Platform-safe compile

- Install Linux build dependencies and fix current `alsa-sys` failure.
- Verify Windows build prerequisites on a Windows runner or VM.
- Gate `macos-private-api`.
- Make `cargo check` pass on all three OSes.
- Keep current macOS functionality unchanged.

### Phase 2: Platform layer extraction

- Create `src-tauri/src/platform`.
- Move existing macOS implementations into `platform/macos.rs`.
- Move common HTTP/time/image helpers into common modules.
- Replace direct calls in `tools.rs`, `sandbox.rs`, and `voice.rs` with platform APIs.
- No feature behavior should change in this phase.

### Phase 3: Low-risk cross-platform tools

- Implement URL opening with opener plugin or platform fallback.
- Implement clipboard through plugin/native crate.
- Implement shell/PATH selection.
- Update tool descriptions dynamically with OS names and limitations.

### Phase 4: Screenshot and app/window listing

- Add common screenshot output pipeline with Rust resizing/JPEG encoding.
- Implement Linux and Windows capture backends.
- Implement Linux and Windows visible-window/process listing.
- Add graceful unsupported messages where the OS blocks the feature.

### Phase 5: Desktop UX hardening

- Test tray, hotkey, overlay, and positioning on all targets.
- Add fallback UI controls for Linux Wayland and any Windows hotkey failures.
- Add configurable push-to-talk vs toggle-to-talk.

### Phase 6: Documentation and packaging

- Rewrite README as multi-OS.
- Add per-OS prerequisite sections.
- Add known limitations section.
- Add packaging instructions.
- Add CI build matrix.

## Acceptance Criteria

Shared:

- `npm run build` passes.
- `cargo check` passes.
- app starts without crashing.
- settings open and persist.
- Ollama/Chatterbox failures are shown as actionable errors.
- Whisper model path can be configured.

macOS:

- existing tray UX still works.
- `Shift+Z` push-to-talk works.
- screenshot, clipboard, URL opening, and app listing work at least as well as before.

Linux:

- app starts on the target desktop environment.
- tray works or fallback window is shown.
- microphone recording works.
- clipboard and URL opening work.
- screenshot either works or returns an accurate limitation.
- global hotkey works or fallback talk mode works.

Windows:

- app starts on Windows 10/11 with WebView2.
- microphone recording works through WASAPI.
- clipboard and URL opening work.
- screenshot works.
- visible-window/process listing works or returns a clear limitation.
- packaging produces a usable NSIS installer or portable dev build.

## Open Decisions

- Which Linux desktop is the first-class target: Linux Mint/Cinnamon, GNOME, KDE, or generic X11/Wayland.
- Whether screenshot capture should use a cross-platform crate or per-OS native implementations.
- Whether clipboard should be implemented through Tauri plugin, Rust crate, or platform CLI fallback.
- Whether push-to-talk is mandatory on Wayland, or toggle-to-talk is acceptable there.
- Whether Chatterbox has an official Windows/Linux deployment path that should be bundled or documented.
- Whether to package Ollama/Chatterbox as external prerequisites only, or support managed sidecars later.
