//! Best-effort local producer. No D-Bus name ownership or lifecycle mutations.
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

pub fn steam_shortcut(key: String) {
    let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") else { return; };
    let path = PathBuf::from(runtime).join("zaman-notifications/control.sock");
    let result = (|| -> std::io::Result<()> {
        let mut stream = UnixStream::connect(path)?;
        stream.set_write_timeout(Some(Duration::from_millis(200)))?;
        stream.set_read_timeout(Some(Duration::from_millis(200)))?;
        let data = serde_json::json!({"action":"event", "source":"Zaman", "key":key,
            "title":"Zaman menu in Steam", "body":"Press Start + Select to open the system menu.",
            "category":"input"});
        stream.write_all(format!("{data}\n").as_bytes())?;
        let mut reply = [0u8; 256];
        let _ = stream.read(&mut reply)?;
        Ok(())
    })();
    if let Err(error) = result {
        eprintln!("Steam shortcut notification unavailable: {error}");
    }
}
