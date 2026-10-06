//! NetworkManager-backed menu operations. All calls run in a blocking worker.
use std::collections::{HashMap, HashSet};
use std::io::{self, Write};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AccessPoint {
    pub ssid: String,
    pub signal: u8,
    pub security: String,
    pub active: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NetworkState {
    pub wifi_enabled: bool,
    pub wifi_device: Option<String>,
    pub wifi_connection: Option<String>,
    pub ethernet_connection: Option<String>,
    pub address: Option<String>,
    pub access_points: Vec<AccessPoint>,
    pub saved_profiles: HashMap<String, String>,
}

#[derive(Debug, PartialEq)]
pub enum NetworkAction {
    Read {
        scan: bool,
    },
    Wifi {
        enabled: bool,
        device: Option<String>,
    },
    Disconnect(String),
    Saved {
        ssid: String,
        uuid: String,
        device: String,
    },
    Connect {
        ssid: String,
        device: String,
        password: Option<String>,
    },
}

fn nmcli(args: &[&str]) -> io::Result<String> {
    let output = Command::new("nmcli").args(args).output()?;
    if !output.status.success() {
        // Never show or log nmcli output: it may contain an SSID or a secret.
        return Err(io::Error::other(format!(
            "NetworkManager operation failed ({})",
            output.status
        )));
    }
    String::from_utf8(output.stdout).map_err(io::Error::other)
}

// nmcli's terse output escapes ':' and '\\' in fields.
fn fields(line: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut field = String::new();
    let mut escaped = false;
    for ch in line.chars() {
        if escaped {
            field.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == ':' {
            result.push(std::mem::take(&mut field));
        } else {
            field.push(ch);
        }
    }
    if escaped {
        field.push('\\');
    }
    result.push(field);
    result
}

fn read(scan: bool) -> io::Result<NetworkState> {
    let wifi_enabled = nmcli(&["-g", "WIFI", "radio"])?.trim() == "enabled";
    let mut state = NetworkState {
        wifi_enabled,
        ..NetworkState::default()
    };
    let devices = nmcli(&[
        "-t",
        "-f",
        "DEVICE,TYPE,STATE,CONNECTION",
        "device",
        "status",
    ])?;
    let mut ethernet_device = None;
    for line in devices.lines() {
        let columns = fields(line);
        if columns.len() < 4 {
            continue;
        }
        let connected = columns[2] == "connected";
        match columns[1].as_str() {
            "wifi" if state.wifi_device.is_none() => {
                state.wifi_device = Some(columns[0].clone());
                if connected {
                    state.wifi_connection = Some(columns[3].clone());
                }
            }
            "ethernet" if connected && state.ethernet_connection.is_none() => {
                state.ethernet_connection = Some(columns[3].clone());
                ethernet_device = Some(columns[0].clone());
            }
            _ => {}
        }
    }
    if let Some(device) = ethernet_device.or_else(|| {
        state.wifi_connection.as_ref()?;
        state.wifi_device.clone()
    }) {
        let value = nmcli(&["-g", "IP4.ADDRESS", "device", "show", &device])?;
        state.address = value
            .lines()
            .find(|line| !line.is_empty())
            .map(str::to_owned);
    }
    if scan && wifi_enabled {
        if let Some(device) = &state.wifi_device {
            let raw = nmcli(&[
                "-t",
                "-f",
                "IN-USE,SSID,SIGNAL,SECURITY",
                "device",
                "wifi",
                "list",
                "ifname",
                device,
                "--rescan",
                "yes",
            ])?;
            let mut seen = HashSet::new();
            for line in raw.lines() {
                let columns = fields(line);
                if columns.len() != 4 || columns[1].is_empty() || !seen.insert(columns[1].clone()) {
                    continue;
                }
                state.access_points.push(AccessPoint {
                    active: columns[0] == "*",
                    ssid: columns[1].clone(),
                    signal: columns[2].parse().unwrap_or(0),
                    security: columns[3].clone(),
                });
            }
            state
                .access_points
                .sort_by(|a, b| b.active.cmp(&a.active).then(b.signal.cmp(&a.signal)));
            state.saved_profiles = saved_wifi_profiles()?;
        }
    }
    Ok(state)
}

// Match the actual Wi-Fi SSID, rather than the user-editable profile name.
// UUIDs uniquely identify a saved profile without reading its secrets.
fn saved_wifi_profiles() -> io::Result<HashMap<String, String>> {
    let profiles = nmcli(&["-t", "-f", "UUID,TYPE", "connection", "show"])?;
    let mut saved = HashMap::new();
    for line in profiles.lines() {
        let columns = fields(line);
        if columns.len() != 2 || !matches!(columns[1].as_str(), "wifi" | "802-11-wireless") {
            continue;
        }
        if let Ok(ssid) = nmcli(&[
            "-g", "802-11-wireless.ssid", "connection", "show", "uuid", &columns[0],
        ]) {
            if let Some(ssid) = ssid.lines().next().filter(|ssid| !ssid.is_empty()) {
                saved.entry(fields(ssid).join(":")).or_insert(columns[0].clone());
            }
        }
    }
    Ok(saved)
}

// --ask keeps the passphrase out of argv and process listings. Input is sent
// through an anonymous pipe, never a shell, log, environment variable or file.
fn connect_with_password(ssid: &str, device: &str, password: String) -> io::Result<()> {
    connect_with_password_using("nmcli", ssid, device, password)
}

fn connect_with_password_using(
    program: &str,
    ssid: &str,
    device: &str,
    mut password: String,
) -> io::Result<()> {
    let mut child = Command::new(program)
        .args([
            "--ask", "--wait", "30", "device", "wifi", "connect", ssid, "ifname", device,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let write_result = (|| {
        let mut input = child.stdin.take().unwrap();
        input.write_all(password.as_bytes())?;
        input.write_all(b"\n")
    })();
    // Reduce lifetime of the menu copy; the child's stdin closes here.
    unsafe {
        password.as_bytes_mut().fill(0);
    }
    if let Err(error) = write_result {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }
    let deadline = Instant::now() + Duration::from_secs(35);
    loop {
        if let Some(status) = child.try_wait()? {
            return if status.success() {
                Ok(())
            } else {
                Err(io::Error::other(
                    "Wi-Fi connection failed; check the password and try again",
                ))
            };
        }
        if Instant::now() >= deadline {
            child.kill()?;
            let _ = child.wait();
            return Err(io::Error::other("Wi-Fi connection timed out"));
        }
        thread::sleep(Duration::from_millis(100));
    }
}

pub fn apply(action: NetworkAction) -> io::Result<NetworkState> {
    match action {
        NetworkAction::Read { scan } => return read(scan),
        NetworkAction::Wifi { enabled, device } => {
            nmcli(&["radio", "wifi", if enabled { "on" } else { "off" }])?;
            if enabled {
                if let Some(device) = device {
                    // "device disconnect" suppresses autoactivation on the device.
                    // Restore it when Wi-Fi is explicitly turned back on.
                    nmcli(&["device", "set", &device, "autoconnect", "yes"])?;
                }
            }
        }
        NetworkAction::Disconnect(device) => {
            nmcli(&["device", "disconnect", &device])?;
        }
        NetworkAction::Saved { uuid, device, .. } => {
            nmcli(&["--wait", "30", "connection", "up", "uuid", &uuid, "ifname", &device])?;
        }
        NetworkAction::Connect {
            ssid,
            device,
            password: Some(password),
        } => connect_with_password(&ssid, &device, password)?,
        NetworkAction::Connect {
            ssid,
            device,
            password: None,
        } => {
            nmcli(&[
                "--wait", "30", "device", "wifi", "connect", &ssid, "ifname", &device,
            ])?;
        }
    }
    read(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn nmcli_terse_escaping() {
        assert_eq!(
            fields(r"wlan0:wifi:connected:My\:Home\\WiFi"),
            ["wlan0", "wifi", "connected", "My:Home\\WiFi"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn password_is_sent_on_stdin_not_in_argv() {
        let path = std::env::temp_dir().join(format!("zaman-nmcli-{}", std::process::id()));
        std::fs::write(&path, b"#!/bin/sh\ncase \" $* \" in *supersecret*) exit 2;; esac\nIFS= read -r secret\n[ \"$secret\" = supersecret ]\n").unwrap();
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&path, permissions).unwrap();
        let result = connect_with_password_using(
            path.to_str().unwrap(),
            "Home Wi-Fi",
            "wlan0",
            "supersecret".into(),
        );
        let _ = std::fs::remove_file(path);
        result.unwrap();
    }
}
