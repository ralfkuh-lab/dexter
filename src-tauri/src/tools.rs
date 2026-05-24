#[cfg(target_os = "macos")]
use base64::{engine::general_purpose::STANDARD, Engine};
use std::process::Command;

/// Capture the screen on macOS using screencapture, resize for vision model.
/// `monitor`: None = active display (with mouse cursor), Some(n) = display index (1-based).
/// Returns the screenshot as a base64-encoded JPEG (resized to max 1280px).
#[cfg(target_os = "macos")]
pub fn take_screenshot(monitor: Option<u32>) -> Result<String, String> {
    let tmp_raw = std::env::temp_dir().join("voice-assistant-screenshot-raw.png");
    let tmp_resized = std::env::temp_dir().join("voice-assistant-screenshot.jpg");
    let raw_str = tmp_raw.to_string_lossy().to_string();
    let resized_str = tmp_resized.to_string_lossy().to_string();

    // Build screencapture args
    let mut args = vec!["-x".to_string(), "-t".to_string(), "png".to_string()];

    // -D<n> selects display: 0 = main (menu bar), 1+ = specific display
    // To get the "active" display (where the mouse cursor is), we use AppleScript
    let display_index = match monitor {
        Some(n) => n,
        None => get_active_display().unwrap_or(1),
    };
    args.push(format!("-D{}", display_index));
    args.push(raw_str.clone());

    let status = Command::new("screencapture")
        .args(&args)
        .status()
        .map_err(|e| format!("Failed to run screencapture: {}", e))?;

    if !status.success() {
        return Err("screencapture exited with non-zero status".to_string());
    }

    // Resize to max 1280px on longest side and convert to JPEG for smaller payload
    let status = Command::new("sips")
        .args([
            "-Z",
            "1280",
            "-s",
            "format",
            "jpeg",
            "-s",
            "formatOptions",
            "70",
            &raw_str,
            "--out",
            &resized_str,
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| format!("Failed to resize screenshot: {}", e))?;

    let _ = std::fs::remove_file(&tmp_raw);

    if !status.success() {
        return Err("sips resize failed".to_string());
    }

    let bytes = std::fs::read(&tmp_resized)
        .map_err(|e| format!("Failed to read resized screenshot: {}", e))?;
    let _ = std::fs::remove_file(&tmp_resized);

    Ok(STANDARD.encode(&bytes))
}

/// Linux-Screenshot: probiert mehrere DE-übliche Tools, da es keine
/// universelle API gibt. Reihenfolge: `grim` (Wayland-wlroots) → DE-eigene
/// Tools (gnome-screenshot, spectacle) → X11-Fallbacks (scrot, maim, import).
/// Multi-Monitor wird ignoriert — wir nehmen, was das Tool als Default liefert
/// (üblicherweise der gesamte virtuelle Desktop).
#[cfg(target_os = "linux")]
pub fn take_screenshot(_monitor: Option<u32>) -> Result<String, String> {
    use base64::{engine::general_purpose::STANDARD, Engine};

    let tmp_raw = std::env::temp_dir().join("voice-assistant-screenshot-raw.png");
    let raw_str = tmp_raw.to_string_lossy().to_string();
    let _ = std::fs::remove_file(&tmp_raw);

    let wayland = std::env::var("WAYLAND_DISPLAY").is_ok()
        || std::env::var("XDG_SESSION_TYPE").ok().as_deref() == Some("wayland");
    let desktop = std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .to_lowercase();

    let mut attempts: Vec<(&str, Vec<String>)> = Vec::new();
    if wayland {
        attempts.push(("grim", vec![raw_str.clone()]));
    }
    if desktop.contains("kde") || desktop.contains("plasma") {
        attempts.push((
            "spectacle",
            vec!["-b".into(), "-n".into(), "-o".into(), raw_str.clone()],
        ));
    }
    // gnome-screenshot funktioniert auf GNOME, Cinnamon, MATE, Unity (X11).
    attempts.push(("gnome-screenshot", vec!["-f".into(), raw_str.clone()]));
    if !wayland {
        // Auf Wayland würde grim oben schon greifen; das hier ist die X11-Strecke.
        attempts.push(("grim", vec![raw_str.clone()]));
    }
    attempts.push(("scrot", vec!["-o".into(), raw_str.clone()]));
    attempts.push(("maim", vec![raw_str.clone()]));
    attempts.push((
        "import",
        vec!["-window".into(), "root".into(), raw_str.clone()],
    ));

    let mut last_err = String::from("no screenshot tool found in PATH (tried grim, gnome-screenshot, spectacle, scrot, maim, import)");
    let mut captured = false;
    for (bin, args) in &attempts {
        let result = Command::new(bin)
            .args(args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        match result {
            Ok(status) if status.success() => {
                if std::fs::metadata(&tmp_raw)
                    .map(|m| m.len() > 0)
                    .unwrap_or(false)
                {
                    captured = true;
                    break;
                }
                last_err = format!("{} exited 0 but wrote no file", bin);
            }
            Ok(status) => {
                last_err = format!("{} exited with {}", bin, status);
            }
            Err(_) => {
                // Binary fehlt — nächsten Kandidaten probieren.
            }
        }
    }

    if !captured {
        return Err(last_err);
    }

    let img = image::open(&tmp_raw).map_err(|e| format!("Failed to decode screenshot: {}", e));
    let _ = std::fs::remove_file(&tmp_raw);
    let img = img?;

    // Auf 1120 px längste Seite — bei Gemma-3n bleibt das in einem SigLIP-
    // Slice (256 Image-Tokens) statt vier, spart Encoding-Zeit und VRAM.
    // Lanczos3 statt Triangle, damit feiner UI-Text scharf bleibt.
    let resized = if img.width().max(img.height()) > 1120 {
        img.resize(1120, 1120, image::imageops::FilterType::Lanczos3)
    } else {
        img
    };

    // Q95 statt Q70 — Screenshots enthalten oft kleine Schrift, JPEG-Artefakte
    // bei Q70 zerstören sonst den OCR-Pfad des Vision-Modells.
    let rgb = resized.to_rgb8();
    let mut jpeg_bytes = Vec::new();
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg_bytes, 95);
    rgb.write_with_encoder(encoder)
        .map_err(|e| format!("JPEG encode failed: {}", e))?;

    Ok(STANDARD.encode(&jpeg_bytes))
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn take_screenshot(_monitor: Option<u32>) -> Result<String, String> {
    Err("Screenshot capture is not implemented for this platform yet.".to_string())
}

/// Get the display index where the mouse cursor currently is.
/// Uses CoreGraphics via a small Python snippet (macOS built-in).
#[cfg(target_os = "macos")]
fn get_active_display() -> Option<u32> {
    // Get mouse location and match to display
    let output = Command::new("python3")
        .args([
            "-c",
            r#"
import Quartz
mouse = Quartz.NSEvent.mouseLocation()
displays = Quartz.CGGetActiveDisplayList(16, None, None)
if displays and displays[1]:
    for i, did in enumerate(displays[1]):
        bounds = Quartz.CGDisplayBounds(did)
        # NSEvent y is flipped (0 = bottom), CGDisplay y is 0 = top
        screen_h = Quartz.CGDisplayPixelsHigh(Quartz.CGMainDisplayID())
        flipped_y = screen_h - mouse.y
        if (bounds.origin.x <= mouse.x < bounds.origin.x + bounds.size.width and
            bounds.origin.y <= flipped_y < bounds.origin.y + bounds.size.height):
            print(i + 1)
            break
    else:
        print(1)
else:
    print(1)
"#,
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;

    let s = String::from_utf8_lossy(&output.stdout);
    s.trim().parse::<u32>().ok()
}

/// Describe a screenshot image using Ollama's vision capabilities.
/// Sends the base64 PNG to the model and asks it to describe what's on screen.
/// Send the JPEG-base64 screenshot to the LLM's vision endpoint.
/// Branches on provider: Ollama-native `/api/chat` for ollama, OpenAI-compatible
/// `/v1/chat/completions` with multipart `content` array for everything else
/// (llama.cpp, vLLM, …). For OpenAI we disable thinking mode — otherwise
/// Gemma-3n-style models burn the entire token budget in `reasoning_content`
/// and return an empty answer.
pub async fn describe_screenshot(
    base_url: &str,
    provider: &str,
    model: &str,
    image_b64: &str,
    question: &str,
) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| e.to_string())?;

    let base = base_url.trim_end_matches('/');

    if provider == "ollama" {
        let body = serde_json::json!({
            "model": model,
            "messages": [{
                "role": "user",
                "content": question,
                "images": [image_b64],
            }],
            "stream": false,
        });

        let resp = client
            .post(format!("{}/api/chat", base))
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Vision request failed: {}", e))?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("Vision error {}: {}", status, text));
        }

        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse vision response: {}", e))?;

        return Ok(json["message"]["content"]
            .as_str()
            .unwrap_or("Could not describe the screenshot.")
            .to_string());
    }

    // OpenAI-compatible providers (llama.cpp, vLLM, sglang, …).
    let url = if base.ends_with("/v1") {
        format!("{}/chat/completions", base)
    } else if base.ends_with("/v1/chat/completions") {
        base.to_string()
    } else {
        format!("{}/v1/chat/completions", base)
    };

    let body = serde_json::json!({
        "model": model,
        "messages": [{
            "role": "user",
            "content": [
                { "type": "text", "text": question },
                {
                    "type": "image_url",
                    "image_url": { "url": format!("data:image/jpeg;base64,{}", image_b64) },
                },
            ],
        }],
        "stream": false,
        "max_tokens": 512,
        "temperature": 0.2,
        "chat_template_kwargs": { "enable_thinking": false },
    });

    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Vision request failed: {}", e))?;

    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Vision error {}: {}", status, text));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse vision response: {}", e))?;

    let content = json["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();
    if !content.is_empty() {
        return Ok(content);
    }
    // Some servers expose the answer only in reasoning_content when thinking
    // mode slips through despite enable_thinking=false. Fall back to that.
    let reasoning = json["choices"][0]["message"]["reasoning_content"]
        .as_str()
        .unwrap_or("")
        .trim();
    if !reasoning.is_empty() {
        return Ok(reasoning.to_string());
    }
    Ok("Could not describe the screenshot.".to_string())
}

/// Read the system clipboard text on macOS using pbpaste.
#[cfg(target_os = "macos")]
pub fn read_clipboard() -> Result<String, String> {
    let output = Command::new("pbpaste")
        .output()
        .map_err(|e| format!("Failed to run pbpaste: {}", e))?;

    if !output.status.success() {
        return Err("pbpaste exited with non-zero status".to_string());
    }

    String::from_utf8(output.stdout).map_err(|e| format!("Clipboard not valid UTF-8: {}", e))
}

#[cfg(target_os = "linux")]
pub fn read_clipboard() -> Result<String, String> {
    let wayland = std::env::var("WAYLAND_DISPLAY").is_ok();
    let candidates: Vec<(&str, Vec<&str>)> = if wayland {
        vec![
            ("wl-paste", vec!["--no-newline"]),
            ("xclip", vec!["-selection", "clipboard", "-o"]),
        ]
    } else {
        vec![
            ("xclip", vec!["-selection", "clipboard", "-o"]),
            ("wl-paste", vec!["--no-newline"]),
        ]
    };

    for (cmd, args) in candidates {
        if let Ok(output) = Command::new(cmd).args(args).output() {
            if output.status.success() {
                return String::from_utf8(output.stdout)
                    .map_err(|e| format!("Clipboard not valid UTF-8: {}", e));
            }
        }
    }

    Err("No supported clipboard helper found. Install wl-clipboard or xclip.".to_string())
}

#[cfg(target_os = "windows")]
pub fn read_clipboard() -> Result<String, String> {
    let output = Command::new("powershell")
        .args(["-NoProfile", "-Command", "Get-Clipboard"])
        .output()
        .map_err(|e| format!("Failed to run PowerShell clipboard command: {}", e))?;

    if !output.status.success() {
        return Err("PowerShell clipboard command exited with non-zero status".to_string());
    }

    String::from_utf8(output.stdout).map_err(|e| format!("Clipboard not valid UTF-8: {}", e))
}

/// Open a URL in the default browser.
#[cfg(target_os = "macos")]
pub fn open_url(url: &str) -> Result<String, String> {
    let status = Command::new("open")
        .arg(url)
        .status()
        .map_err(|e| format!("Failed to open URL: {}", e))?;

    if status.success() {
        Ok(format!("Opened {} in the default browser.", url))
    } else {
        Err("Failed to open URL".to_string())
    }
}

#[cfg(target_os = "linux")]
pub fn open_url(url: &str) -> Result<String, String> {
    let status = Command::new("xdg-open")
        .arg(url)
        .status()
        .map_err(|e| format!("Failed to open URL: {}", e))?;

    if status.success() {
        Ok(format!("Opened {} in the default browser.", url))
    } else {
        Err("Failed to open URL".to_string())
    }
}

#[cfg(target_os = "windows")]
pub fn open_url(url: &str) -> Result<String, String> {
    let status = Command::new("cmd")
        .args(["/C", "start", "", url])
        .status()
        .map_err(|e| format!("Failed to open URL: {}", e))?;

    if status.success() {
        Ok(format!("Opened {} in the default browser.", url))
    } else {
        Err("Failed to open URL".to_string())
    }
}

/// Get current date, time, and day of week.
pub fn get_current_time() -> String {
    let now = chrono::Local::now();
    now.format("%A, %B %e, %Y at %I:%M %p").to_string()
}

/// Fetch a URL and return its text content (HTML stripped to readable text).
pub async fn web_fetch(url: &str) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent("Mozilla/5.0 (X11; Linux x86_64; rv:121.0) Gecko/20100101 Firefox/121.0")
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;

    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Fetch failed: {}", e))?;

    let status = resp.status();
    if !status.is_success() {
        return Err(format!("HTTP {}", status));
    }

    let html = resp
        .text()
        .await
        .map_err(|e| format!("Failed to read body: {}", e))?;

    // Strip HTML to plain text
    let text = strip_html(&html);

    // Truncate to avoid flooding context
    let max_len = 6000;
    let total_chars = text.chars().count();
    if total_chars > max_len {
        Ok(format!(
            "{}...\n(truncated, {} total chars)",
            truncate_chars(&text, max_len),
            total_chars
        ))
    } else {
        Ok(text)
    }
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::truncate_chars;

    #[test]
    fn truncate_chars_does_not_split_multibyte_codepoints() {
        assert_eq!(truncate_chars("äöü😀xyz", 4), "äöü😀");
    }
}

/// Naive HTML-to-text: strip tags, decode common entities, collapse whitespace.
fn strip_html(html: &str) -> String {
    // Remove script and style blocks entirely
    let mut s = html.to_string();
    for tag in &["script", "style", "noscript", "svg"] {
        loop {
            let open = format!("<{}", tag);
            let close = format!("</{}>", tag);
            if let Some(start) = s.to_lowercase().find(&open) {
                if let Some(end) = s.to_lowercase()[start..].find(&close) {
                    s = format!("{}{}", &s[..start], &s[start + end + close.len()..]);
                } else {
                    break;
                }
            } else {
                break;
            }
        }
    }

    // Replace block elements with newlines
    let block_tags = [
        "</p>",
        "</div>",
        "</li>",
        "</h1>",
        "</h2>",
        "</h3>",
        "</h4>",
        "</h5>",
        "</h6>",
        "<br>",
        "<br/>",
        "<br />",
        "</tr>",
        "</blockquote>",
    ];
    for tag in block_tags {
        s = s.replace(tag, "\n");
    }

    // Strip remaining tags
    let mut result = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        if ch == '<' {
            in_tag = true;
        } else if ch == '>' {
            in_tag = false;
        } else if !in_tag {
            result.push(ch);
        }
    }

    // Decode common entities
    let result = result
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
        .replace("&#x27;", "'")
        .replace("&#x2F;", "/");

    // Collapse whitespace: multiple spaces → one, multiple newlines → two
    let mut cleaned = String::with_capacity(result.len());
    let mut prev_newline = 0;
    let mut prev_space = false;
    for ch in result.chars() {
        if ch == '\n' || ch == '\r' {
            prev_newline += 1;
            prev_space = false;
            if prev_newline <= 2 {
                cleaned.push('\n');
            }
        } else if ch.is_whitespace() {
            prev_newline = 0;
            if !prev_space {
                cleaned.push(' ');
                prev_space = true;
            }
        } else {
            prev_newline = 0;
            prev_space = false;
            cleaned.push(ch);
        }
    }

    cleaned.trim().to_string()
}

/// List running applications on macOS.
#[cfg(target_os = "macos")]
pub fn list_running_apps() -> Result<String, String> {
    let output = Command::new("osascript")
        .args([
            "-e",
            r#"tell application "System Events" to get name of every process whose background only is false"#,
        ])
        .output()
        .map_err(|e| format!("Failed to list apps: {}", e))?;

    if !output.status.success() {
        return Err("Could not list running apps".to_string());
    }

    String::from_utf8(output.stdout).map_err(|e| format!("Output not valid UTF-8: {}", e))
}

#[cfg(target_os = "linux")]
pub fn list_running_apps() -> Result<String, String> {
    if let Ok(output) = Command::new("wmctrl").arg("-lx").output() {
        if output.status.success() {
            return String::from_utf8(output.stdout)
                .map_err(|e| format!("Output not valid UTF-8: {}", e));
        }
    }

    let output = Command::new("ps")
        .args(["-eo", "comm=", "--sort=comm"])
        .output()
        .map_err(|e| format!("Failed to list processes: {}", e))?;

    if !output.status.success() {
        return Err("Could not list running processes".to_string());
    }

    String::from_utf8(output.stdout).map_err(|e| format!("Output not valid UTF-8: {}", e))
}

#[cfg(target_os = "windows")]
pub fn list_running_apps() -> Result<String, String> {
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "Get-Process | Select-Object -ExpandProperty ProcessName | Sort-Object -Unique",
        ])
        .output()
        .map_err(|e| format!("Failed to list processes: {}", e))?;

    if !output.status.success() {
        return Err("Could not list running processes".to_string());
    }

    String::from_utf8(output.stdout).map_err(|e| format!("Output not valid UTF-8: {}", e))
}
