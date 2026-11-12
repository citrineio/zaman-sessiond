//! PipeWire playback output discovery and WirePlumber controls.
//! All process calls run on the menu's blocking audio worker.
use serde_json::Value;
use std::io;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AudioState {
    pub percent: u8,
    pub muted: bool,
}
impl AudioState {
    pub fn displayed_percent(self) -> u8 { self.percent }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AudioOutput {
    pub id: u32,
    pub serial: String,
    pub name: String,
    pub description: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AudioSnapshot {
    pub level: Option<AudioState>,
    pub outputs: Vec<AudioOutput>,
    pub default_name: Option<String>,
}
impl AudioSnapshot {
    pub fn current(&self) -> Option<&AudioOutput> {
        self.outputs.iter().find(|output| Some(&output.name) == self.default_name.as_ref())
    }
    fn resolve(&self, selected: &AudioOutput) -> Result<&AudioOutput> {
        self.outputs.iter().find(|output| output.name == selected.name && output.serial == selected.serial)
            .ok_or_else(|| io::Error::other("The selected audio output is no longer available. Refresh and select it again.").into())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AudioAction {
    Read,
    Set(u8),
    SetMute(bool),
    SetDefault(AudioOutput),
}

pub fn parse_volume(output: &str) -> Result<AudioState> {
    let value = output.split_whitespace().nth(1)
        .ok_or_else(|| io::Error::other("missing volume"))?;
    let volume: f32 = value.parse()?;
    if !volume.is_finite() || volume < 0.0 {
        return Err(io::Error::other("invalid volume").into());
    }
    Ok(AudioState {
        percent: (volume * 100.0).round().clamp(0.0, 100.0) as u8,
        muted: output.contains("[MUTED]"),
    })
}

fn command(program: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(program).env("LC_ALL", "C").args(args).output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!("{program} failed: {}", String::from_utf8_lossy(&output.stderr))).into());
    }
    Ok(String::from_utf8(output.stdout)?)
}

// pw-dump is JSON on the Debian PipeWire version; don't parse wpctl's
// decorative status tree or require the newer wpctl list subcommand.
fn parse_outputs(raw: &str) -> Result<AudioSnapshot> {
    let objects: Vec<Value> = serde_json::from_str(raw)?;
    let mut snapshot = AudioSnapshot::default();
    for object in &objects {
        if object["type"] == "PipeWire:Interface:Node" {
            let props = &object["info"]["props"];
            if props["media.class"] != "Audio/Sink" { continue; }
            let Some(id) = object["id"].as_u64().and_then(|id| u32::try_from(id).ok()) else { continue; };
            let Some(name) = props["node.name"].as_str().filter(|name| !name.is_empty()) else { continue; };
            let serial = match &props["object.serial"] {
                Value::String(serial) if !serial.is_empty() => serial.clone(),
                Value::Number(serial) => serial.to_string(),
                _ => continue,
            };
            let description = props["node.description"].as_str()
                .or_else(|| props["node.nick"].as_str()).filter(|name| !name.is_empty()).unwrap_or(name)
                .chars().filter(|c| !c.is_control()).take(80).collect();
            snapshot.outputs.push(AudioOutput { id, serial, name: name.into(), description });
        } else if object["type"] == "PipeWire:Interface:Metadata"
            && object["props"]["metadata.name"] == "default" {
            if let Some(entries) = object["metadata"].as_array() {
                for entry in entries {
                    if entry["subject"] == 0 && entry["key"] == "default.audio.sink" {
                        snapshot.default_name = entry["value"]["name"].as_str().map(str::to_owned);
                    }
                }
            }
        }
    }
    snapshot.outputs.sort_by(|a, b| a.description.cmp(&b.description).then(a.name.cmp(&b.name)));
    Ok(snapshot)
}

fn outputs() -> Result<AudioSnapshot> {
    parse_outputs(&command("pw-dump", &["-N"])?)
}
fn read() -> Result<AudioSnapshot> {
    let mut snapshot = outputs()?;
    if let Some(current) = snapshot.current() {
        let id = current.id.to_string();
        snapshot.level = Some(parse_volume(&command("wpctl", &["get-volume", &id])?)?);
    }
    Ok(snapshot)
}

pub fn apply(action: AudioAction) -> Result<AudioSnapshot> {
    match action {
        AudioAction::Read => return read(),
        AudioAction::SetDefault(selected) => {
            // Re-resolve identity immediately before applying: PipeWire IDs
            // can be recycled when a USB/Bluetooth/HDMI output disappears.
            let snapshot = outputs()?;
            let id = snapshot.resolve(&selected)?.id.to_string();
            command("wpctl", &["set-default", &id])?;
            let deadline = Instant::now() + Duration::from_millis(1500);
            loop {
                let snapshot = read()?;
                if snapshot.current().is_some_and(|current| current.name == selected.name
                    && current.serial == selected.serial) {
                    return Ok(snapshot);
                }
                if Instant::now() >= deadline {
                    return Err(io::Error::other("The selected audio output did not become the default. Refresh and try again.").into());
                }
                thread::sleep(Duration::from_millis(75));
            }
        }
        action => {
            let snapshot = outputs()?;
            let current = snapshot.current().ok_or_else(|| io::Error::other("No default playback output is available."))?;
            let id = current.id.to_string();
            match action {
                AudioAction::Set(percent) => { command("wpctl", &["set-volume", &id, &format!("{}%", percent.min(100))])?; }
                AudioAction::SetMute(muted) => { command("wpctl", &["set-mute", &id, if muted { "1" } else { "0" }])?; }
                _ => unreachable!(),
            }
        }
    }
    read()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DUMP: &str = r#"[
      {"id":42,"type":"PipeWire:Interface:Node","info":{"props":{"object.serial":101,"media.class":"Audio/Sink","node.name":"hdmi","node.description":"HDMI monitor"}}},
      {"id":43,"type":"PipeWire:Interface:Node","info":{"props":{"object.serial":"102","media.class":"Audio/Sink","node.name":"usb","node.nick":"USB headphones"}}},
      {"id":44,"type":"PipeWire:Interface:Node","info":{"props":{"object.serial":103,"media.class":"Audio/Source","node.name":"mic"}}},
      {"id":45,"type":"PipeWire:Interface:Node","info":{"props":{"object.serial":104,"media.class":"Stream/Output/Audio","node.name":"game"}}},
      {"id":10,"type":"PipeWire:Interface:Metadata","props":{"metadata.name":"default"},"metadata":[
        {"subject":0,"key":"default.configured.audio.sink","value":{"name":"usb"}},
        {"subject":0,"key":"default.audio.sink","value":{"name":"hdmi"}}
      ]}
    ]"#;

    #[test]
    fn discovery_uses_active_default_and_excludes_inputs_and_streams() {
        let snapshot = parse_outputs(DUMP).unwrap();
        assert_eq!(snapshot.outputs.len(), 2);
        assert_eq!(snapshot.current().unwrap().name, "hdmi");
        assert_eq!(snapshot.outputs[1].description, "USB headphones");
        assert_eq!(snapshot.outputs[0].serial, "101");
        assert_eq!(snapshot.outputs[1].serial, "102");
        assert!(snapshot.level.is_none());
    }

    #[test]
    fn recycled_node_id_cannot_select_another_output() {
        let snapshot = parse_outputs(DUMP).unwrap();
        let mut selected = snapshot.current().unwrap().clone();
        selected.serial = "old instance".into();
        assert!(snapshot.resolve(&selected).is_err());
        selected.serial = "101".into();
        selected.id = 999; // Identity wins over a cached node ID.
        assert_eq!(snapshot.resolve(&selected).unwrap().id, 42);
    }

    #[test]
    fn missing_default_does_not_choose_the_first_sink() {
        let snapshot = parse_outputs(&DUMP.replace("\"name\":\"hdmi\"", "\"name\":\"removed\"")).unwrap();
        assert!(snapshot.current().is_none());
        assert!(parse_outputs("{}").is_err());
        assert_eq!(parse_volume("Volume: 0.42 [MUTED]").unwrap(), AudioState { percent: 42, muted: true });
        assert!(parse_volume("Volume: NaN").is_err());
    }
}
