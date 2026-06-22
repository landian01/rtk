//! Shared finalization for compacted command output.

use crate::core::raw_cache;

#[derive(Debug, Clone)]
pub struct FinalizedOutput {
    pub output: String,
}

#[derive(Debug, Clone)]
pub struct FinalizeRequest<'a> {
    pub adapter: &'a str,
    pub command: &'a str,
    pub raw_stdout: &'a str,
    pub raw_stderr: &'a str,
    pub filter_input: &'a str,
    pub filtered: String,
    pub exit_code: i32,
    pub elapsed_ms: u64,
    pub emit_metadata: bool,
}

pub fn finalize(mut request: FinalizeRequest<'_>) -> FinalizedOutput {
    let filtered = std::mem::take(&mut request.filtered);
    let decision = compact_decision(request.filter_input, filtered);

    if !request.emit_metadata {
        return FinalizedOutput {
            output: decision.output,
        };
    }

    if !metadata_enabled() {
        return finalize_default(request, decision);
    }

    let raw_cache = raw_cache::save(
        request.adapter,
        request.command,
        request.raw_stdout,
        request.raw_stderr,
        request.exit_code,
    )
    .ok();
    let raw_id = raw_cache.map(|entry| entry.id);
    let marker = compact_marker(
        request.adapter,
        decision.raw_bytes,
        decision.shown_bytes,
        request.elapsed_ms,
        request.exit_code,
        decision.passthrough,
        raw_id.as_deref(),
    );
    let output = append_marker(decision.output, &marker);

    FinalizedOutput { output }
}

fn finalize_default(request: FinalizeRequest<'_>, decision: CompactDecision) -> FinalizedOutput {
    if decision.passthrough {
        return FinalizedOutput {
            output: decision.output,
        };
    }

    let output = match raw_cache::save(
        request.adapter,
        request.command,
        request.raw_stdout,
        request.raw_stderr,
        request.exit_code,
    ) {
        Ok(entry) => append_marker(decision.output, &raw_id_marker(&entry.id)),
        Err(_) => decision.output,
    };

    FinalizedOutput { output }
}

fn metadata_enabled() -> bool {
    std::env::var("RTK_COMPACT_METADATA").is_ok_and(|value| value.eq_ignore_ascii_case("always"))
}

pub fn append_metadata_only(
    adapter: &str,
    command: &str,
    raw_stdout: &str,
    raw_stderr: &str,
    shown_output: &str,
    exit_code: i32,
    elapsed_ms: u64,
) -> String {
    let raw_bytes = raw_stdout.len() + raw_stderr.len();
    let shown_bytes = shown_output.len();
    let raw_cache = if metadata_enabled() || has_meaningful_savings(raw_bytes, shown_bytes) {
        raw_cache::save(adapter, command, raw_stdout, raw_stderr, exit_code).ok()
    } else {
        None
    };
    let raw_id = raw_cache.as_ref().map(|entry| entry.id.as_str());

    if !metadata_enabled() {
        return raw_id.map(raw_id_marker).unwrap_or_default();
    }

    compact_marker(
        adapter,
        raw_bytes,
        shown_bytes,
        elapsed_ms,
        exit_code,
        false,
        raw_id,
    )
}

fn raw_id_marker(id: &str) -> String {
    format!("[raw: rtk raw show {}]", id)
}

#[derive(Debug, Clone)]
struct CompactDecision {
    output: String,
    passthrough: bool,
    raw_bytes: usize,
    shown_bytes: usize,
}

fn compact_decision(raw: &str, filtered: String) -> CompactDecision {
    let raw_bytes = raw.len();
    let filtered_bytes = filtered.len();

    if raw_bytes == 0 {
        return CompactDecision {
            output: String::new(),
            passthrough: true,
            raw_bytes,
            shown_bytes: 0,
        };
    }

    if !has_meaningful_savings(raw_bytes, filtered_bytes) {
        CompactDecision {
            output: raw.to_string(),
            passthrough: true,
            raw_bytes,
            shown_bytes: raw_bytes,
        }
    } else {
        CompactDecision {
            output: filtered,
            passthrough: false,
            raw_bytes,
            shown_bytes: filtered_bytes,
        }
    }
}

fn has_meaningful_savings(raw_bytes: usize, shown_bytes: usize) -> bool {
    const MIN_SAVINGS_BYTES: usize = 64;
    const MIN_SAVINGS_PERCENT: f64 = 5.0;

    if raw_bytes == 0 || shown_bytes >= raw_bytes {
        return false;
    }

    let saved_bytes = raw_bytes - shown_bytes;
    let saved_pct = (saved_bytes as f64 / raw_bytes as f64) * 100.0;
    saved_bytes >= MIN_SAVINGS_BYTES && saved_pct >= MIN_SAVINGS_PERCENT
}

fn append_marker(output: String, marker: &str) -> String {
    if output.ends_with('\n') {
        format!("{}{}", output, marker)
    } else {
        format!("{}\n{}", output, marker)
    }
}

fn compact_marker(
    adapter: &str,
    raw_bytes: usize,
    shown_bytes: usize,
    elapsed_ms: u64,
    exit_code: i32,
    passthrough: bool,
    raw_id: Option<&str>,
) -> String {
    let saved_pct = if raw_bytes > shown_bytes && raw_bytes > 0 {
        ((raw_bytes - shown_bytes) as f64 / raw_bytes as f64) * 100.0
    } else {
        0.0
    };
    let raw_ref = raw_id.unwrap_or("unavailable");
    format!(
        "[compact: adapter={} raw={} shown={} saved={:.0}% time={}ms exit={} passthrough={} raw={}]",
        adapter,
        format_bytes(raw_bytes),
        format_bytes(shown_bytes),
        saved_pct,
        elapsed_ms,
        exit_code,
        passthrough,
        raw_ref
    )
}

fn format_bytes(bytes: usize) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;

    if bytes >= 1024 * 1024 {
        format!("{:.1}MB", bytes as f64 / MB)
    } else if bytes >= 1024 {
        format!("{:.1}KB", bytes as f64 / KB)
    } else {
        format!("{}B", bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn test_compact_decision_keeps_smaller_output() {
        let raw = format!("{}\n", "noise line\n".repeat(40));
        let decision = compact_decision(&raw, "summary\n".to_string());
        assert!(!decision.passthrough);
        assert_eq!(decision.output, "summary\n");
        assert_eq!(decision.raw_bytes, raw.len());
        assert_eq!(decision.shown_bytes, "summary\n".len());
    }

    #[test]
    fn test_compact_decision_passthrough_when_no_savings() {
        let decision = compact_decision("short\n", "longer output\n".to_string());
        assert!(decision.passthrough);
        assert_eq!(decision.output, "short\n");
        assert_eq!(decision.raw_bytes, 6);
        assert_eq!(decision.shown_bytes, 6);
    }

    #[test]
    fn test_compact_decision_empty_output_stays_empty() {
        let decision = compact_decision("", String::new());
        assert!(decision.passthrough);
        assert_eq!(decision.output, "");
        assert_eq!(decision.raw_bytes, 0);
        assert_eq!(decision.shown_bytes, 0);
    }

    #[test]
    fn test_compact_decision_passthrough_for_tiny_savings_on_short_output() {
        let decision = compact_decision("11.7.0\n", "11.7.0".to_string());
        assert!(decision.passthrough);
        assert_eq!(decision.output, "11.7.0\n");
        assert_eq!(decision.raw_bytes, 7);
        assert_eq!(decision.shown_bytes, 7);
    }

    #[test]
    fn test_compact_decision_passthrough_for_tiny_savings_on_larger_output() {
        let raw = format!("{}\n", "status line\n".repeat(40));
        let filtered = raw.trim_end().to_string();
        let decision = compact_decision(&raw, filtered);
        assert!(decision.passthrough);
        assert_eq!(decision.output, raw);
    }

    #[test]
    fn test_compact_marker_formats_metadata() {
        let marker = compact_marker("git.diff", 10_240, 1_024, 42, 0, false, Some("abc123"));
        assert!(marker.contains("adapter=git.diff"));
        assert!(marker.contains("raw=10.0KB"));
        assert!(marker.contains("shown=1.0KB"));
        assert!(marker.contains("saved=90%"));
        assert!(marker.contains("time=42ms"));
        assert!(marker.contains("exit=0"));
        assert!(marker.contains("passthrough=false"));
        assert!(marker.contains("raw=abc123"));
    }

    #[test]
    fn test_append_marker_respects_existing_newline() {
        assert_eq!(append_marker("body\n".to_string(), "[m]"), "body\n[m]");
        assert_eq!(append_marker("body".to_string(), "[m]"), "body\n[m]");
    }

    #[test]
    fn test_finalize_skips_marker_for_passthrough_by_default() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("RTK_COMPACT_METADATA");
        let finalized = finalize(FinalizeRequest {
            adapter: "test",
            command: "test command",
            raw_stdout: "short\n",
            raw_stderr: "",
            filter_input: "short\n",
            filtered: "short".to_string(),
            exit_code: 0,
            elapsed_ms: 1,
            emit_metadata: true,
        });

        assert_eq!(finalized.output, "short\n");
    }

    #[test]
    fn test_finalize_emits_only_raw_id_for_compressed_output_by_default() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::env::remove_var("RTK_COMPACT_METADATA");
        std::env::set_var("COMPACT_CACHE_DIR", dir.path());
        let raw = format!("{}\n", "noise line\n".repeat(40));
        let finalized = finalize(FinalizeRequest {
            adapter: "test",
            command: "test command",
            raw_stdout: &raw,
            raw_stderr: "",
            filter_input: &raw,
            filtered: "summary\n".to_string(),
            exit_code: 0,
            elapsed_ms: 1,
            emit_metadata: true,
        });

        assert!(finalized.output.starts_with("summary\n"));
        assert!(finalized.output.contains("[raw: rtk raw show "));
        assert!(!finalized.output.contains("[compact:"));
        std::env::remove_var("COMPACT_CACHE_DIR");
    }

    #[test]
    fn test_finalize_emits_marker_when_metadata_always() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("RTK_COMPACT_METADATA", "always");
        std::env::set_var("COMPACT_CACHE_DIR", dir.path());
        let raw = format!("{}\n", "noise line\n".repeat(40));
        let finalized = finalize(FinalizeRequest {
            adapter: "test",
            command: "test command",
            raw_stdout: &raw,
            raw_stderr: "",
            filter_input: &raw,
            filtered: "summary\n".to_string(),
            exit_code: 0,
            elapsed_ms: 1,
            emit_metadata: true,
        });

        assert!(finalized.output.contains("[compact: adapter=test"));
        std::env::remove_var("RTK_COMPACT_METADATA");
        std::env::remove_var("COMPACT_CACHE_DIR");
    }
}
