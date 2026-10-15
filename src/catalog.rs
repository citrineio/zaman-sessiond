//! Curated IDs are read from the shared file for every new authorization.
use serde_json::Value;
use std::{fs::File, io::{self, Read}, path::{Path, PathBuf}};

pub const DEFAULT_PATH: &str = "/usr/local/share/zaman/catalog.json";
const MAX_BYTES: u64 = 1024 * 1024;
#[derive(Clone, Debug)]
pub struct Game { pub id: String, pub name: String, pub enabled: bool }
type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
fn invalid() -> io::Error { io::Error::new(io::ErrorKind::InvalidData, "invalid shared game catalog") }
pub fn path() -> Result<PathBuf> {
    let path = std::env::var_os("ZAMAN_CATALOG_PATH").map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_PATH));
    if !path.is_absolute() { return Err(invalid().into()); }
    Ok(path)
}
fn valid_id(id: &str) -> bool {
    let mut parts = id.split('.');
    let first = parts.next().unwrap_or("");
    id.len() <= 255 && first.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
        && id.split('.').count() >= 3
        && id.split('.').all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'))
}
pub fn parse(bytes: &[u8]) -> Result<Vec<Game>> {
    if bytes.len() > MAX_BYTES as usize { return Err(invalid().into()); }
    let root: Value = serde_json::from_slice(bytes)?;
    let object = root.as_object().ok_or_else(invalid)?;
    if object.len() != 2 || root.get("schema_version").and_then(Value::as_f64) != Some(1.0) { return Err(invalid().into()); }
    let rows = root.get("games").and_then(Value::as_array).ok_or_else(invalid)?;
    if rows.len() > 512 { return Err(invalid().into()); }
    let mut seen = std::collections::HashSet::new();
    let mut games = Vec::new();
    for row in rows {
        let obj = row.as_object().ok_or_else(invalid)?;
        if obj.keys().any(|k| !["id", "enabled", "name", "genre", "summary", "tag", "package_logo"].contains(&k.as_str())) { return Err(invalid().into()); }
        let id = row.get("id").and_then(Value::as_str).ok_or_else(invalid)?;
        if !valid_id(id) || !seen.insert(id.to_owned()) { return Err(invalid().into()); }
        let enabled = match row.get("enabled") { None => true, Some(v) => v.as_bool().ok_or_else(invalid)? };
        let mut name = id.rsplit('.').next().unwrap_or(id).to_owned();
        for (field, max) in [("name",160), ("genre",80), ("summary",600), ("tag",80), ("package_logo",512)] {
            if let Some(v) = row.get(field) {
                let text = v.as_str().ok_or_else(invalid)?;
                if text.chars().count() > max || text.chars().any(|c| c <= '\u{1f}' || c == '\u{7f}') { return Err(invalid().into()); }
                let text = text.trim();
                if field == "name" && !text.is_empty() { name = text.to_owned(); }
                if field == "package_logo" && !text.is_empty() && (text.starts_with('/') || text.contains('\\') || text.split('/').any(|p| p.is_empty() || p == "." || p == "..")) { return Err(invalid().into()); }
            }
        }
        games.push(Game { id: id.into(), name, enabled });
    }
    Ok(games)
}
pub fn load_from(path: &Path) -> Result<Vec<Game>> {
    let mut bytes = Vec::new();
    File::open(path)?.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    parse(&bytes)
}
pub fn load() -> Result<Vec<Game>> { load_from(&path()?) }
pub fn require(app: &str) -> Result<String> {
    load()?.into_iter().find(|g| g.enabled && g.id == app).map(|g| g.name)
        .ok_or_else(|| io::Error::new(io::ErrorKind::PermissionDenied, "application is outside the enabled curated catalog").into())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn minimal_and_invalid() {
        let games = parse(br#"{"schema_version":1,"games":[{"id":"org.example.Game"}]}"#).unwrap();
        assert_eq!(games[0].name,"Game"); assert!(games[0].enabled);
        for data in [r#"{"schema_version":true,"games":[]}"#, r#"{"schema_version":1,"games":[{"id":"../bad"}]}"#, r#"{"schema_version":1,"games":[{"id":"org.example.Game","name":"bad\nlaunch: unsafe"}]}"#, r#"{"schema_version":1,"games":[{"id":"org.example.Game","package_logo":"../logo.png"}]}"#] {
            assert!(parse(data.as_bytes()).is_err());
        }
    }
    #[test] fn reload_from_disk() {
        let path = std::env::temp_dir().join(format!("zaman-catalog-test-{}.json", std::process::id()));
        std::fs::write(&path, br#"{"schema_version":1,"games":[{"id":"org.example.New"}]}"#).unwrap();
        assert!(load_from(&path).unwrap()[0].enabled);
        std::fs::write(&path, br#"{"schema_version":1,"games":[{"id":"org.example.New","enabled":false}]}"#).unwrap();
        assert!(!load_from(&path).unwrap()[0].enabled);
        std::fs::write(&path, b"{").unwrap(); assert!(load_from(&path).is_err());
        std::fs::remove_file(&path).unwrap(); assert!(load_from(&path).is_err());
    }
    #[test] fn canonical_and_duplicates() {
        assert_eq!(parse(include_bytes!("../catalog/games.json")).unwrap().len(),5);
        assert!(parse(br#"{"schema_version":1,"games":[{"id":"org.example.Game"},{"id":"org.example.Game","enabled":false}]}"#).is_err());
    }
}
