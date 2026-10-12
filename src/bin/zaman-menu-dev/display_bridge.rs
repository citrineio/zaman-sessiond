//! Display worker owned by the development input/focus lease, not by a QML page.
use super::{display::{DisplayManager, Mode, Output}, emit};
use serde_json::{json, Value};
use std::sync::{atomic::{AtomicBool, AtomicU64, Ordering}, mpsc, Arc};
use std::time::{Duration, Instant};

pub struct DisplayBridge {
    active: Arc<AtomicBool>,
    generation: Arc<AtomicU64>,
    busy: Arc<AtomicBool>,
    sender: mpsc::Sender<(u64, Value)>,
    manager: DisplayManager,
}
impl DisplayBridge {
    pub async fn new() -> super::Result<Self> {
        let active = Arc::new(AtomicBool::new(false));
        let generation = Arc::new(AtomicU64::new(0));
        let valid_active = active.clone();
        let valid_generation = generation.clone();
        let manager = tokio::task::spawn_blocking(move || DisplayManager::new(move |g|
            valid_active.load(Ordering::SeqCst) && valid_generation.load(Ordering::SeqCst) == g)).await?;
        let busy = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = mpsc::channel::<(u64, Value)>();
        let worker = manager.clone();
        let worker_active = active.clone();
        let worker_generation = generation.clone();
        let worker_busy = busy.clone();
        std::thread::spawn(move || {
            for (generation, command) in receiver {
                let result = if !worker_active.load(Ordering::SeqCst)
                    || worker_generation.load(Ordering::SeqCst) != generation {
                    Err("Display page closed or changed".into())
                } else {
                    request(&worker, generation, &command)
                };
                let (state, error) = match result {
                    Ok(state) => (Some(state), String::new()),
                    Err(error) => (worker.status().ok(), error),
                };
                // Release the slot before publishing; QML may immediately reply.
                worker_busy.store(false, Ordering::SeqCst);
                // A persistent helper can begin another menu session while an
                // old page request finishes. Never publish a stale response.
                if !worker_active.load(Ordering::SeqCst)
                    || worker_generation.load(Ordering::SeqCst) != generation { continue; }
                if emit(json!({"type":"display", "id":command["id"],
                    "state":state, "error":error, "busy":false})).is_err() { break; }
            }
        });
        Ok(Self { active, generation, busy, sender, manager })
    }
    pub fn handle(&self, command: &Value) -> super::Result<()> {
        let Some(action) = command["action"].as_str() else { return Ok(()) };
        if action == "display-open" {
            // Invalidate work from the previous visit before accepting new work.
            self.active.store(false, Ordering::SeqCst);
            self.generation.fetch_add(1, Ordering::SeqCst);
            self.active.store(true, Ordering::SeqCst);
            return Ok(());
        }
        if action == "display-leave" {
            self.deactivate();
            return Ok(());
        }
        if !matches!(action, "display-status" | "display-preview" | "display-keep" | "display-revert") {
            return Ok(());
        }
        if self.busy.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
            return emit(json!({"type":"display", "id":command["id"], "busy":true}));
        }
        let generation = self.generation.load(Ordering::SeqCst);
        if self.sender.send((generation, command.clone())).is_err() {
            self.busy.store(false, Ordering::SeqCst);
            emit(json!({"type":"display", "id":command["id"], "error":"Display worker stopped", "busy":false}))?;
        }
        Ok(())
    }
    pub fn deactivate(&self) { self.active.store(false, Ordering::SeqCst); }
    pub async fn shutdown(&self) -> super::Result<()> {
        self.deactivate();
        let manager = self.manager.clone();
        tokio::task::spawn_blocking(move || {
            let deadline = Instant::now() + Duration::from_secs(8);
            loop {
                match manager.shutdown() {
                    Ok(()) => return Ok(()),
                    Err(error) if !manager.pending() || Instant::now() >= deadline => {
                        return Err(super::error::message(format!("Display recovery: {error}")));
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(250)),
                }
            }
        }).await?
    }
}
fn token(command: &Value) -> std::result::Result<u64, String> {
    command["token"].as_str().and_then(|s| s.parse().ok())
        .ok_or_else(|| "Invalid display preview token (must be a decimal string)".into())
}
fn request(manager: &DisplayManager, generation: u64, command: &Value)
    -> std::result::Result<super::display::DisplayState, String> {
    match command["action"].as_str() {
        Some("display-status") => manager.status(),
        Some("display-preview") => {
            let output: Output = serde_json::from_value(command["output"].clone())
                .map_err(|e| format!("Invalid output identity: {e}"))?;
            let mode: Mode = serde_json::from_value(command["mode"].clone())
                .map_err(|e| format!("Invalid display mode: {e}"))?;
            manager.preview_checked(generation, output, mode)
        }
        Some("display-keep") => manager.confirm(generation, token(command)?),
        Some("display-revert") => manager.revert(generation, token(command)?),
        _ => Err("Unknown display action".into()),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn token_bridge_requires_an_exact_string_not_a_rounded_number() {
        assert_eq!(token(&json!({"token":"18446744073709551615"})).unwrap(), u64::MAX);
        assert!(token(&json!({"token":9007199254740992u64})).is_err());
        assert!(token(&json!({"token":"18446744073709551616"})).is_err());
    }
}
