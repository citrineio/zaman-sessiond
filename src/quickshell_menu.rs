//! Active menu IPC. Work runs only on explicit requests, never on an idle timer.
use crate::api::MenuAction;
use crate::error::{message, Result};
use serde::Deserialize;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Deserialize)]
pub struct ShellStatus {
    pub status: String,
    #[serde(rename = "sessionKey")]
    pub session_key: String,
    pub generation: u64,
    #[serde(default = "conservative_input_owner", rename = "inputOwned")] pub input_owned: bool,
}

fn conservative_input_owner() -> bool { true }

impl ShellStatus {
    pub fn presented_or_pending(&self) -> bool {
        matches!(self.status.as_str(), "open" | "opening" | "closing" | "busy")
    }
}

fn configuration() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("ZAMAN_MENU_QML") {
        return Ok(path.into());
    }
    let home = std::env::var_os("HOME").ok_or_else(|| message("HOME is unavailable for menu IPC"))?;
    Ok(PathBuf::from(home).join("zaman-quickshell-prototype/qml/shell.qml"))
}

fn ipc_blocking(config: PathBuf, method: &'static str, arguments: Vec<String>) -> Result<String> {
    let mut child = Command::new("qs")
        .arg("-p").arg(config).args(["ipc", "call", "menu", method]).args(arguments)
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn()?;
    let deadline = Instant::now() + Duration::from_millis(1500);
    let exit = loop {
        match child.try_wait() {
            Ok(Some(exit)) => break exit,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(5)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(message("Quickshell menu IPC timed out"));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error.into());
            }
        }
    };
    let mut output = String::new();
    if let Some(stdout) = child.stdout.take() {
        stdout.take(65536).read_to_string(&mut output)?;
    }
    if !exit.success() {
        return Err(message("Resident Quickshell is unavailable; check zaman-quickshell.service"));
    }
    Ok(output)
}

async fn ipc(method: &'static str) -> Result<String> {
    ipc_with_arguments(method, Vec::new()).await
}

async fn ipc_with_arguments(method: &'static str, arguments: Vec<String>) -> Result<String> {
    let config = configuration()?;
    tokio::task::spawn_blocking(move || ipc_blocking(config, method, arguments)).await?
}

pub fn log(message: &str) {
    use std::os::unix::fs::OpenOptionsExt;
    if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") {
        if let Ok(mut file) = std::fs::OpenOptions::new().append(true).create(true).mode(0o600)
            .open(PathBuf::from(runtime).join("zaman-menu-timing.log"))
        {
            let _ = writeln!(file, "pid={} [menu-guide] {}", std::process::id(), message.replace('\n', " "));
        }
    }
}

pub async fn close_session(session: String) -> Result<()> {
    // Check the key and close atomically in the renderer; a separate status
    // check would race with F12/Start+Select or a renderer restart.
    let result = match ipc_with_arguments("closeSession", vec![session]).await {
        Ok(output) => acknowledge(&output),
        Err(error) => Err(error),
    };
    match &result {
        Ok(()) => log("Guide release processed"),
        Err(error) => log(&format!("Guide release failed: {error}")),
    }
    result
}

pub fn parse_status(output: &str) -> Result<ShellStatus> {
    let status = output.lines().rev().find_map(|line| serde_json::from_str::<ShellStatus>(line.trim()).ok())
        .ok_or_else(|| message("Quickshell menu returned an incompatible status; load the Guide update"))?;
    if !matches!(status.status.as_str(), "hidden" | "open" | "opening" | "closing" | "busy")
        || status.session_key.is_empty()
    {
        return Err(message("Invalid Quickshell menu status"));
    }
    Ok(status)
}

pub async fn status() -> Result<ShellStatus> {
    parse_status(&ipc("timings").await?)
}

pub async fn dispatch(action: MenuAction) -> Result<()> {
    let method = match action {
        MenuAction::Open => "open",
        MenuAction::Close => "close",
        MenuAction::Toggle => "toggle",
    };
    let result = match ipc(method).await {
        Ok(output) => acknowledge(&output),
        Err(error) => Err(error),
    };
    match &result {
        Ok(()) => log(&format!("{method} accepted")),
        Err(error) => log(&format!("{method} failed: {error}")),
    }
    result
}

fn acknowledge(output: &str) -> Result<()> {
    let response = output.lines().rev().map(str::trim).find(|line|
        matches!(*line, "hidden" | "open" | "opening" | "closing" | "busy" | "exiting-game"));
    match response {
        Some("busy" | "exiting-game") => Err(message("busy: a menu session action is in flight")),
        Some(_) => Ok(()),
        None => Err(message("Quickshell did not acknowledge the menu request")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_requires_current_instance_session_identity() {
        assert!(parse_status(r#"{"status":"open"}"#).is_err());
        assert!(parse_status(r#"{"status":"open","sessionKey":"","generation":1}"#).is_err());
        let state = parse_status("INFO: old log line\n{\"status\":\"closing\",\"sessionKey\":\"instance:2\",\"generation\":2}\n").unwrap();
        assert!(state.presented_or_pending());
        assert_eq!(state.session_key, "instance:2");
        assert_eq!(state.generation, 2);
    }

    #[test]
    fn status_distinguishes_restoration_from_hidden() {
        for state in ["opening", "open", "closing", "busy"] {
            let status = parse_status(&format!(r#"{{"status":"{state}","sessionKey":"i:1","generation":1}}"#)).unwrap();
            assert!(status.presented_or_pending());
        }
        assert!(!parse_status(r#"{"status":"hidden","sessionKey":"i:0","generation":0}"#).unwrap().presented_or_pending());
    }

    #[test]
    fn request_requires_a_known_acknowledgement_and_blocks_busy_actions() {
        assert!(acknowledge("INFO: cli log\nopening\n").is_ok());
        assert!(acknowledge("hidden\n").is_ok());
        assert!(acknowledge("busy\n").is_err());
        assert!(acknowledge("exiting-game\n").is_err());
        assert!(acknowledge("unknown\n").is_err());
    }
}
