use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use serde::Deserialize;


use crate::session_manager::{SessionMessage, SessionMeta};
use super::utils::{truncate_summary, parse_timestamp_to_ms};

const PROVIDER_ID: &str = "antigravity";

#[derive(Debug, Deserialize, Clone)]
struct HistoryLine {
    display: Option<String>,
    timestamp: Option<i64>,
    workspace: Option<String>,
    #[serde(rename = "conversationId")]
    conversation_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TranscriptLine {
    source: Option<String>,
    #[serde(rename = "type")]
    line_type: Option<String>,
    content: Option<String>,
    created_at: Option<serde_json::Value>, // Robust to accept both strings and numbers
}

pub fn parse_antigravity_source_path(source_path: &str) -> Result<(PathBuf, String), String> {
    let parts: Vec<&str> = source_path.split("#conversationId=").collect();
    if parts.len() != 2 {
        return Err(format!("Invalid Antigravity source path: {source_path}"));
    }
    Ok((PathBuf::from(parts[0]), parts[1].to_string()))
}

pub fn scan_sessions() -> Vec<SessionMeta> {
    let history_path = crate::antigravity_config::get_antigravity_dir().join("history.jsonl");
    if !history_path.exists() {
        return Vec::new();
    }

    let file = match File::open(&history_path) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };

    let reader = BufReader::new(file);
    let mut sessions_map: HashMap<String, Vec<HistoryLine>> = HashMap::new();

    for line_result in reader.lines() {
        let line = match line_result {
            Ok(l) => l,
            Err(_) => continue,
        };

        if let Ok(entry) = serde_json::from_str::<HistoryLine>(&line) {
            if let Some(ref conv_id) = entry.conversation_id {
                if !conv_id.trim().is_empty() {
                    sessions_map.entry(conv_id.clone()).or_default().push(entry);
                }
            }
        }
    }

    let mut sessions = Vec::new();

    for (conv_id, entries) in sessions_map {
        if entries.is_empty() {
            continue;
        }

        // Sort entries by timestamp ascending to find the first and last
        let mut sorted_entries = entries;
        sorted_entries.sort_by_key(|e| e.timestamp.unwrap_or(0));

        let first = &sorted_entries[0];
        let last = &sorted_entries[sorted_entries.len() - 1];

        let title = first
            .display
            .as_deref()
            .map(|s| truncate_summary(s, 160));

        let created_at = first.timestamp;
        let last_active_at = last.timestamp;
        let project_dir = first.workspace.clone();
        let source_path = format!("{}#conversationId={}", history_path.to_string_lossy(), conv_id);

        sessions.push(SessionMeta {
            provider_id: PROVIDER_ID.to_string(),
            session_id: conv_id.clone(),
            title: title.clone(),
            summary: title,
            project_dir,
            created_at,
            last_active_at: last_active_at.or(created_at),
            source_path: Some(source_path),
            resume_command: Some(format!("agy --conversation {conv_id}")),
        });
    }

    sessions
}

fn extract_user_request(content: &str) -> String {
    if let Some(start) = content.find("<USER_REQUEST>") {
        if let Some(end) = content.find("</USER_REQUEST>") {
            let req_content = &content[start + "<USER_REQUEST>".len()..end];
            return req_content.trim().to_string();
        }
    }
    content.trim().to_string()
}

pub fn load_messages(path: &Path) -> Result<Vec<SessionMessage>, String> {
    let path_str = path.to_string_lossy();
    let (history_path, conv_id) = parse_antigravity_source_path(&path_str)?;

    // Validate path to prevent path traversal / unauthorized file reading in production
    #[cfg(not(test))]
    {
        let expected_history_path = crate::antigravity_config::get_antigravity_dir().join("history.jsonl");
        if history_path != expected_history_path {
            return Err("Access denied: source history path is invalid.".to_string());
        }
    }

    // 1. Try to read from transcript.jsonl
    let transcript_path = crate::antigravity_config::get_antigravity_dir()
        .join("brain")
        .join(&conv_id)
        .join(".system_generated")
        .join("logs")
        .join("transcript.jsonl");

    if transcript_path.exists() {
        if let Ok(file) = File::open(&transcript_path) {
            let reader = BufReader::new(file);
            let mut result = Vec::new();

            for line_result in reader.lines() {
                let line = match line_result {
                    Ok(l) => l,
                    Err(_) => continue,
                };
                if let Ok(line_val) = serde_json::from_str::<TranscriptLine>(&line) {
                    let ts = line_val.created_at.as_ref().and_then(|raw_ts| {
                        parse_timestamp_to_ms(raw_ts)
                    });

                    if line_val.line_type.as_deref() == Some("USER_INPUT") {
                        if let Some(ref raw_content) = line_val.content {
                            let content = extract_user_request(raw_content);
                            if !content.is_empty() {
                                result.push(SessionMessage {
                                    role: "user".to_string(),
                                    content,
                                    ts,
                                });
                            }
                        }
                    } else if line_val.source.as_deref() == Some("MODEL")
                        && line_val.line_type.as_deref() == Some("PLANNER_RESPONSE")
                    {
                        if let Some(ref raw_content) = line_val.content {
                            if !raw_content.trim().is_empty() {
                                result.push(SessionMessage {
                                    role: "assistant".to_string(),
                                    content: raw_content.trim().to_string(),
                                    ts,
                                });
                            }
                        }
                    }
                }
            }

            if !result.is_empty() {
                // Sort by timestamp
                result.sort_by_key(|m| m.ts.unwrap_or(0));
                return Ok(result);
            }
        }
    }

    // 2. Fallback: Parse display entries from history.jsonl
    let file = File::open(&history_path)
        .map_err(|e| format!("Failed to open history.jsonl: {e}"))?;
    let reader = BufReader::new(file);
    let mut result = Vec::new();

    for line_result in reader.lines() {
        let line = line_result.map_err(|e| format!("Failed to read history line: {e}"))?;
        if let Ok(entry) = serde_json::from_str::<HistoryLine>(&line) {
            if entry.conversation_id.as_deref() == Some(&conv_id) {
                if let Some(content) = entry.display {
                    if !content.trim().is_empty() {
                        result.push(SessionMessage {
                            role: "user".to_string(),
                            content,
                            ts: entry.timestamp,
                        });
                    }
                }
            }
        }
    }

    // Sort by timestamp
    result.sort_by_key(|m| m.ts.unwrap_or(0));

    Ok(result)
}

pub fn delete_session(path: &Path, session_id: &str) -> Result<bool, String> {
    let path_str = path.to_string_lossy();
    let (history_path, conv_id) = parse_antigravity_source_path(&path_str)?;

    if conv_id != session_id {
        return Err(format!(
            "Session ID mismatch: expected {session_id}, found {conv_id}"
        ));
    }

    // Validate path to prevent path traversal / unauthorized file overwrites in production
    #[cfg(not(test))]
    {
        let expected_history_path = crate::antigravity_config::get_antigravity_dir().join("history.jsonl");
        if history_path != expected_history_path {
            return Err("Access denied: source history path is invalid.".to_string());
        }
    }

    // 1. Remove entries from history.jsonl
    let file = File::open(&history_path)
        .map_err(|e| format!("Failed to open history.jsonl: {e}"))?;
    let reader = BufReader::new(file);
    let mut kept_lines = Vec::new();

    for line_result in reader.lines() {
        let line = line_result.map_err(|e| format!("Failed to read line: {e}"))?;
        let should_keep = match serde_json::from_str::<HistoryLine>(&line) {
            Ok(entry) => entry.conversation_id.as_deref() != Some(session_id),
            Err(_) => true,
        };
        if should_keep {
            kept_lines.push(line);
        }
    }

    // Atomic Write
    let dir = history_path.parent().ok_or("Invalid path")?;
    let temp_path = dir.join(format!("history.jsonl.tmp.{}", std::process::id()));
    {
        let mut out_file = File::create(&temp_path)
            .map_err(|e| format!("Failed to create temporary history file: {e}"))?;
        for line in kept_lines {
            writeln!(out_file, "{}", line).map_err(|e| format!("Failed to write line: {e}"))?;
        }
        out_file.sync_all().map_err(|e| format!("Failed to sync file: {e}"))?;
    } // File is closed here
    
    std::fs::rename(&temp_path, &history_path)
        .map_err(|e| format!("Failed to replace history.jsonl: {e}"))?;

    // 2. Remove the binary .pb file from conversations directory
    let pb_path = crate::antigravity_config::get_antigravity_dir()
        .join("conversations")
        .join(format!("{}.pb", session_id));
    if pb_path.exists() {
        let _ = std::fs::remove_file(pb_path);
    }

    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use std::io::Write;

    #[test]
    fn test_extract_user_request() {
        assert_eq!(
            extract_user_request("<USER_REQUEST>\nhello world\n</USER_REQUEST>\nsome metadata"),
            "hello world"
        );
        assert_eq!(
            extract_user_request("hello plain text"),
            "hello plain text"
        );
    }

    #[test]
    fn test_load_messages_fallback_to_history() {
        let temp = tempdir().expect("tempdir");
        let history_path = temp.path().join("history.jsonl");
        let mut f = File::create(&history_path).expect("create file");
        writeln!(f, r#"{{"display":"hello","timestamp":1000,"workspace":"/workspace","conversationId":"conv1"}}"#).expect("write");
        drop(f);

        let source_path = format!("{}#conversationId=conv1", history_path.to_string_lossy());
        let msgs = load_messages(Path::new(&source_path)).expect("load");
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[0].content, "hello");
        assert_eq!(msgs[0].ts, Some(1000));
    }
}
