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
            "-Z", "1280",
            "-s", "format", "jpeg",
            "-s", "formatOptions", "70",
            &raw_str,
            "--out", &resized_str,
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

#[cfg(not(target_os = "macos"))]
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
pub async fn describe_screenshot(
    ollama_url: &str,
    model: &str,
    image_b64: &str,
    question: &str,
) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| e.to_string())?;

    let body = serde_json::json!({
        "model": model,
        "messages": [{
            "role": "user",
            "content": question,
            "images": [image_b64]
        }],
        "stream": false
    });

    let resp = client
        .post(format!("{}/api/chat", ollama_url))
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Ollama vision request failed: {}", e))?;

    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Ollama vision error {}: {}", status, text));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse vision response: {}", e))?;

    Ok(json["message"]["content"]
        .as_str()
        .unwrap_or("Could not describe the screenshot.")
        .to_string())
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
        vec![("wl-paste", vec!["--no-newline"]), ("xclip", vec!["-selection", "clipboard", "-o"])]
    } else {
        vec![("xclip", vec!["-selection", "clipboard", "-o"]), ("wl-paste", vec!["--no-newline"])]
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
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36")
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
    if text.len() > max_len {
        Ok(format!("{}...\n(truncated, {} total chars)", &text[..max_len], text.len()))
    } else {
        Ok(text)
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
    let block_tags = ["</p>", "</div>", "</li>", "</h1>", "</h2>", "</h3>", "</h4>", "</h5>", "</h6>", "<br>", "<br/>", "<br />", "</tr>", "</blockquote>"];
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
        .args(["-NoProfile", "-Command", "Get-Process | Select-Object -ExpandProperty ProcessName | Sort-Object -Unique"])
        .output()
        .map_err(|e| format!("Failed to list processes: {}", e))?;

    if !output.status.success() {
        return Err("Could not list running processes".to_string());
    }

    String::from_utf8(output.stdout).map_err(|e| format!("Output not valid UTF-8: {}", e))
}
