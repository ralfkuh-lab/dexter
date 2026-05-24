use std::collections::HashSet;
use std::io::Write;
use std::process::{Command, Output, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Sandbox security level.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
pub enum SandboxMode {
    /// Commands run in a temp workspace with env sanitization + blocklist.
    Guarded,
    /// Commands run inside a Docker container with volume mounts.
    Docker,
}

impl Default for SandboxMode {
    fn default() -> Self {
        Self::Guarded
    }
}

/// Configuration for the sandboxed shell.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SandboxConfig {
    #[serde(default)]
    pub mode: SandboxMode,
    /// Max seconds a command can run before being killed.
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    /// Directories the LLM is allowed to read (guarded mode).
    #[serde(default = "default_readable_paths")]
    pub readable_paths: Vec<String>,
    /// The workspace directory commands run in.
    #[serde(default = "default_workspace")]
    pub workspace: String,
    /// Docker image for Docker mode.
    #[serde(default = "default_docker_image")]
    pub docker_image: String,
    /// Whether network access is allowed in Docker mode.
    #[serde(default = "default_true")]
    pub allow_network: bool,
}

fn default_timeout() -> u64 {
    30
}
fn default_true() -> bool {
    true
}
fn default_readable_paths() -> Vec<String> {
    vec![
        "~/Documents".to_string(),
        "~/Desktop".to_string(),
        "~/Downloads".to_string(),
        "~/Projects".to_string(),
    ]
}
fn default_workspace() -> String {
    dirs::home_dir()
        .map(|h| {
            h.join(".voice-assistant-sandbox")
                .to_string_lossy()
                .to_string()
        })
        .unwrap_or_else(|| "/tmp/voice-assistant-sandbox".to_string())
}
fn default_docker_image() -> String {
    "ubuntu:24.04".to_string()
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            mode: SandboxMode::default(),
            timeout_secs: default_timeout(),
            readable_paths: default_readable_paths(),
            workspace: default_workspace(),
            docker_image: default_docker_image(),
            allow_network: true,
        }
    }
}

// ── Blocked command patterns ──

/// Commands/patterns that are never allowed regardless of mode.
const BLOCKED_COMMANDS: &[&str] = &["sudo", "su", "doas", "pkexec"];

const BLOCKED_PATTERNS: &[&str] = &[
    // Destructive filesystem ops
    "rm -rf /",
    "rm -rf ~",
    "rm -rf $HOME",
    "rm -rf /*",
    "rm -rf ~/*",
    // Disk/partition ops
    "mkfs",
    "dd if=",
    "diskutil eraseDisk",
    "diskutil partitionDisk",
    // System control
    "shutdown",
    "reboot",
    "halt",
    "poweroff",
    "launchctl unload",
    "csrutil",
    // Fork bomb variants
    ":(){ :|:& };:",
    ".(){.|.&};.",
    // Credential/keychain theft
    "security find-generic-password",
    "security find-internet-password",
    "security dump-keychain",
    // Network exfiltration of sensitive files
    "curl.*~/.ssh",
    "curl.*/.aws",
    "curl.*/.env",
    "wget.*~/.ssh",
    // Chmod dangerous
    "chmod -R 777 /",
    "chmod -R 777 ~",
    // Kill all
    "killall -9",
    "kill -9 -1",
    "pkill -9",
];

/// Env vars to strip from the sandbox environment.
const STRIPPED_ENV_VARS: &[&str] = &[
    "AWS_ACCESS_KEY_ID",
    "AWS_SECRET_ACCESS_KEY",
    "AWS_SESSION_TOKEN",
    "GITHUB_TOKEN",
    "GITLAB_TOKEN",
    "OPENAI_API_KEY",
    "ANTHROPIC_API_KEY",
    "OPENROUTER_API_KEY",
    "DATABASE_URL",
    "REDIS_URL",
    "SECRET_KEY",
    "PRIVATE_KEY",
    "SSH_AUTH_SOCK",
    "GPG_AGENT_INFO",
    "HOMEBREW_GITHUB_API_TOKEN",
    "NPM_TOKEN",
    "CARGO_REGISTRY_TOKEN",
    "DOCKER_PASSWORD",
    "NERVE_API_KEY",
];

/// Audit logger for sandbox commands.
pub struct AuditLog {
    path: std::path::PathBuf,
}

impl AuditLog {
    pub fn new() -> Self {
        let path = dirs::config_dir()
            .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".config"))
            .join("voice-assistant")
            .join("sandbox-audit.log");
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        Self { path }
    }

    pub fn log(&self, command: &str, result: &str, blocked: bool) {
        let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
        let status = if blocked { "BLOCKED" } else { "EXECUTED" };
        let entry = format!(
            "[{}] {} | cmd: {} | result: {}\n",
            timestamp,
            status,
            command,
            if result.chars().count() > 200 {
                format!("{}...", truncate_chars(result, 200))
            } else {
                result.to_string()
            }
        );
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        {
            let _ = f.write_all(entry.as_bytes());
        }
    }
}

/// Validate a command against the blocklist. Returns Err with reason if blocked.
pub fn validate_command(command: &str) -> Result<(), String> {
    let lower = command.to_lowercase();
    let trimmed = command.trim();

    // Check if command starts with a blocked command
    let first_word = trimmed.split_whitespace().next().unwrap_or("");
    // Strip path prefix (e.g. /usr/bin/sudo -> sudo)
    let base_cmd = first_word.rsplit('/').next().unwrap_or(first_word);

    if BLOCKED_COMMANDS.contains(&base_cmd) {
        return Err(format!(
            "Command '{}' is blocked — elevated privileges are not allowed in the sandbox.",
            base_cmd
        ));
    }

    // Check for pipe chains that start with a blocked command
    for segment in command
        .split('|')
        .chain(command.split("&&"))
        .chain(command.split(';'))
    {
        let seg_first = segment.trim().split_whitespace().next().unwrap_or("");
        let seg_base = seg_first.rsplit('/').next().unwrap_or(seg_first);
        if BLOCKED_COMMANDS.contains(&seg_base) {
            return Err(format!(
                "Command '{}' in pipeline is blocked — elevated privileges are not allowed.",
                seg_base
            ));
        }
    }

    // Check for blocked patterns
    for pattern in BLOCKED_PATTERNS {
        if lower.contains(&pattern.to_lowercase()) {
            return Err(format!(
                "Command blocked — matches dangerous pattern: '{}'",
                pattern
            ));
        }
    }

    Ok(())
}

/// Build a sanitized environment for the sandbox.
fn build_safe_env() -> Vec<(String, String)> {
    let stripped: HashSet<&str> = STRIPPED_ENV_VARS.iter().copied().collect();
    let mut env: Vec<(String, String)> = std::env::vars()
        .filter(|(k, _)| !stripped.contains(k.as_str()))
        .collect();

    // Override PATH to only include standard locations
    let safe_path = platform_safe_path();
    env.retain(|(k, _)| k != "PATH");
    env.push(("PATH".to_string(), safe_path.to_string()));

    // Set HOME
    if let Some(home) = dirs::home_dir() {
        env.retain(|(k, _)| k != "HOME");
        env.push(("HOME".to_string(), home.to_string_lossy().to_string()));
    }

    env
}

#[cfg(target_os = "macos")]
fn platform_safe_path() -> &'static str {
    "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
}

#[cfg(target_os = "linux")]
fn platform_safe_path() -> &'static str {
    "/usr/local/bin:/usr/bin:/bin:/usr/local/sbin:/usr/sbin:/sbin"
}

#[cfg(target_os = "windows")]
fn platform_safe_path() -> &'static str {
    r"C:\Windows\System32;C:\Windows;C:\Windows\System32\WindowsPowerShell\v1.0"
}

#[cfg(target_os = "macos")]
fn platform_shell() -> (&'static str, [&'static str; 1]) {
    ("zsh", ["-c"])
}

#[cfg(target_os = "linux")]
fn platform_shell() -> (&'static str, [&'static str; 1]) {
    ("sh", ["-c"])
}

#[cfg(target_os = "windows")]
fn platform_shell() -> (&'static str, [&'static str; 1]) {
    ("powershell", ["-Command"])
}

/// Execute a command in guarded mode.
fn run_guarded(command: &str, config: &SandboxConfig) -> Result<String, String> {
    // Ensure workspace exists
    let _ = std::fs::create_dir_all(&config.workspace);

    let env = build_safe_env();

    let (shell, shell_args) = platform_shell();
    let mut cmd = Command::new(shell);
    cmd.args(shell_args)
        .arg(command)
        .current_dir(&config.workspace)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_clear()
        .envs(env);

    let output = run_with_timeout(&mut cmd, Duration::from_secs(config.timeout_secs))
        .map_err(|e| format!("Failed to execute: {}", e))?;

    format_output(&output)
}

/// Execute a command in Docker mode.
fn run_docker(command: &str, config: &SandboxConfig) -> Result<String, String> {
    // Check if Docker is available
    let docker_check = Command::new("docker")
        .args(["info"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    match docker_check {
        Ok(status) if status.success() => {}
        _ => return Err(
            "Docker is not running. Switch to Guarded mode in settings or start Docker Desktop."
                .to_string(),
        ),
    }

    let _ = std::fs::create_dir_all(&config.workspace);

    let mut args = vec![
        "run".to_string(),
        "--rm".to_string(),
        // Resource limits
        "--memory=512m".to_string(),
        "--cpus=1.0".to_string(),
        "--pids-limit=100".to_string(),
        // No privileges
        "--security-opt=no-new-privileges".to_string(),
        "--read-only".to_string(),
        // Writable workspace
        "-v".to_string(),
        format!("{}:/workspace", config.workspace),
        "-w".to_string(),
        "/workspace".to_string(),
        // Writable tmp
        "--tmpfs".to_string(),
        "/tmp:rw,noexec,nosuid,size=64m".to_string(),
    ];

    // Mount readable paths as read-only
    for path in &config.readable_paths {
        let expanded = path.replace('~', &dirs::home_dir().unwrap_or_default().to_string_lossy());
        if std::path::Path::new(&expanded).exists() {
            let mount_point = format!(
                "/mnt/{}",
                std::path::Path::new(&expanded)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
            );
            args.push("-v".to_string());
            args.push(format!("{}:{}:ro", expanded, mount_point));
        }
    }

    // Network control
    if !config.allow_network {
        args.push("--network=none".to_string());
    }

    args.push(config.docker_image.clone());
    args.push("sh".to_string());
    args.push("-c".to_string());
    args.push(command.to_string());

    // Docker timeout = config timeout + 5s grace for container startup
    let timeout = config.timeout_secs + 5;
    let mut cmd = Command::new("docker");
    cmd.args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = run_with_timeout(&mut cmd, Duration::from_secs(timeout))
        .map_err(|e| format!("Docker exec failed: {}", e))?;

    format_output(&output)
}

fn run_with_timeout(command: &mut Command, timeout: Duration) -> Result<Output, String> {
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let started = Instant::now();

    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().map_err(|e| e.to_string()),
            Ok(None) => {
                if started.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!(
                        "Command timed out after {}s and was killed",
                        timeout.as_secs()
                    ));
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// Format command output, truncating if needed.
fn format_output(output: &std::process::Output) -> Result<String, String> {
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    let max_len = 4000;
    let mut result = String::new();

    if !stdout.is_empty() {
        if stdout.chars().count() > max_len {
            result.push_str(&truncate_chars(&stdout, max_len));
            result.push_str("\n... (output truncated)");
        } else {
            result.push_str(&stdout);
        }
    }

    if !stderr.is_empty() {
        if !result.is_empty() {
            result.push_str("\n\nSTDERR:\n");
        }
        let stderr_trunc = if stderr.chars().count() > 1000 {
            format!("{}... (truncated)", truncate_chars(&stderr, 1000))
        } else {
            stderr
        };
        result.push_str(&stderr_trunc);
    }

    if !output.status.success() {
        result = format!(
            "Exit code: {}\n{}",
            output.status.code().unwrap_or(-1),
            result
        );
    }

    if result.is_empty() {
        result = "(no output)".to_string();
    }

    Ok(result)
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::{run_guarded, truncate_chars, SandboxConfig};

    #[test]
    fn truncate_chars_does_not_split_multibyte_codepoints() {
        assert_eq!(truncate_chars("äöü😀xyz", 4), "äöü😀");
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn guarded_timeout_kills_command() {
        let config = SandboxConfig {
            timeout_secs: 1,
            workspace: std::env::temp_dir().to_string_lossy().to_string(),
            ..SandboxConfig::default()
        };

        let err = run_guarded("sleep 3", &config).expect_err("command should time out");

        assert!(err.contains("timed out"));
        assert!(err.contains("killed"));
    }
}

/// The main entry point — validate, execute, and audit.
pub fn execute(
    command: &str,
    config: &SandboxConfig,
    audit: &Mutex<AuditLog>,
) -> Result<String, String> {
    // Validate first
    if let Err(reason) = validate_command(command) {
        if let Ok(log) = audit.lock() {
            log.log(command, &reason, true);
        }
        return Err(reason);
    }

    // Execute based on mode
    let result = match config.mode {
        SandboxMode::Guarded => run_guarded(command, config),
        SandboxMode::Docker => run_docker(command, config),
    };

    // Audit
    if let Ok(log) = audit.lock() {
        match &result {
            Ok(output) => log.log(command, output, false),
            Err(e) => log.log(command, e, false),
        }
    }

    result
}
