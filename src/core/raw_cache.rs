//! Raw output cache for compacted command runs.

use anyhow::{bail, Context, Result};
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

const CACHE_ENV: &str = "COMPACT_CACHE_DIR";
const CACHE_DIR: &str = "compact-cache";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawCapture {
    pub version: u8,
    pub id: String,
    pub timestamp: String,
    pub adapter: String,
    pub command: String,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone)]
pub struct SaveResult {
    pub id: String,
}

pub fn cache_dir() -> PathBuf {
    if let Ok(dir) = std::env::var(CACHE_ENV) {
        return PathBuf::from(dir);
    }

    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".codex")
        .join(CACHE_DIR)
}

pub fn save(
    adapter: &str,
    command: &str,
    stdout: &str,
    stderr: &str,
    exit_code: i32,
) -> Result<SaveResult> {
    let timestamp = Utc::now().to_rfc3339_opts(SecondsFormat::Nanos, true);
    let id = capture_id(adapter, command, stdout, stderr, exit_code, &timestamp);
    let capture = RawCapture {
        version: 1,
        id: id.clone(),
        timestamp,
        adapter: adapter.to_string(),
        command: command.to_string(),
        exit_code,
        stdout: stdout.to_string(),
        stderr: stderr.to_string(),
    };

    let dir = cache_dir();
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create raw cache dir: {}", dir.display()))?;
    let path = dir.join(format!("{}.json", id));
    let bytes = serde_json::to_vec_pretty(&capture)?;
    std::fs::write(&path, bytes)
        .with_context(|| format!("Failed to write raw cache entry: {}", path.display()))?;

    Ok(SaveResult { id })
}

pub fn load(id: &str) -> Result<RawCapture> {
    validate_id(id)?;
    let path = cache_dir().join(format!("{}.json", id));
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read raw cache entry: {}", path.display()))?;
    serde_json::from_str(&text)
        .with_context(|| format!("Failed to parse raw cache entry: {}", path.display()))
}

fn validate_id(id: &str) -> Result<()> {
    if id.len() == 12 && id.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Ok(());
    }

    bail!("Raw cache id must be a 12-character hex string")
}

fn capture_id(
    adapter: &str,
    command: &str,
    stdout: &str,
    stderr: &str,
    exit_code: i32,
    timestamp: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(adapter.as_bytes());
    hasher.update([0]);
    hasher.update(command.as_bytes());
    hasher.update([0]);
    hasher.update(stdout.as_bytes());
    hasher.update([0]);
    hasher.update(stderr.as_bytes());
    hasher.update([0]);
    hasher.update(exit_code.to_le_bytes());
    hasher.update(timestamp.as_bytes());
    let digest = format!("{:x}", hasher.finalize());
    digest[..12].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn test_save_and_load_round_trip() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var(CACHE_ENV, dir.path());

        let saved = save("git.diff", "git diff", "stdout text", "stderr text", 7).unwrap();
        assert!(cache_dir().join(format!("{}.json", saved.id)).exists());

        let loaded = load(&saved.id).unwrap();
        assert_eq!(loaded.adapter, "git.diff");
        assert_eq!(loaded.command, "git diff");
        assert_eq!(loaded.exit_code, 7);
        assert_eq!(loaded.stdout, "stdout text");
        assert_eq!(loaded.stderr, "stderr text");

        std::env::remove_var(CACHE_ENV);
    }

    #[test]
    fn test_load_rejects_invalid_id() {
        assert!(load("../not-a-cache-id").is_err());
        assert!(load("abc123").is_err());
    }
}
