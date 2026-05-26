//! Antigravity CLI (`agy`) 配置文件管理
//!
//! 负责读写 Antigravity CLI 的本地配置文件：
//! - `~/.gemini/antigravity-cli/settings.json` — 主设置
//! - `~/.gemini/antigravity-cli/mcp_config.json` — MCP Server 配置
//! - `~/.gemini/antigravity-cli/skills/` — Skills 目录（由 services/skill.rs 统一管理）

use crate::config::{get_home_dir, write_text_file};
use crate::error::AppError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

// ─────────────────────────────────────────────
// 路径工具
// ─────────────────────────────────────────────

/// 获取 Antigravity CLI 配置目录：`~/.gemini/antigravity-cli/`
pub fn get_antigravity_dir() -> PathBuf {
    get_home_dir().join(".gemini").join("antigravity-cli")
}

/// 获取 Antigravity CLI 主设置文件路径
pub fn get_antigravity_settings_path() -> PathBuf {
    get_antigravity_dir().join("settings.json")
}

/// 获取 Antigravity CLI MCP 配置文件路径
pub fn get_antigravity_mcp_config_path() -> PathBuf {
    get_antigravity_dir().join("mcp_config.json")
}

/// 获取 Antigravity CLI Skills 目录路径
pub fn get_antigravity_skills_dir() -> PathBuf {
    get_antigravity_dir().join("skills")
}

// ─────────────────────────────────────────────
// settings.json
// ─────────────────────────────────────────────

/// Antigravity CLI `settings.json` 的反序列化结构
///
/// 采用宽松模式：已知字段显式映射，其余字段保留在 `extra` 中以防止写回时丢失。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AntigravitySettings {
    /// 当前选用的模型名称
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    /// 颜色方案（"dark" / "light" / "system"）
    #[serde(rename = "colorScheme", default, skip_serializing_if = "Option::is_none")]
    pub color_scheme: Option<String>,

    /// 受信任工作区列表
    #[serde(rename = "trustedWorkspaces", default, skip_serializing_if = "Vec::is_empty")]
    pub trusted_workspaces: Vec<String>,

    /// API Base URL (用于自定义端点/代理)
    #[serde(rename = "base_url", alias = "baseUrl", default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,

    /// API Key 对应的环境变量名称 (例如 ANTIGRAVITY_API_KEY)
    #[serde(rename = "env_key", alias = "envKey", default, skip_serializing_if = "Option::is_none")]
    pub env_key: Option<String>,

    /// 保留其他未映射的字段，避免写回时丢失
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// 读取 Antigravity CLI 主设置
pub fn read_antigravity_settings() -> Result<AntigravitySettings, AppError> {
    let path = get_antigravity_settings_path();

    if !path.exists() {
        log::info!("Antigravity settings.json 不存在，返回默认值");
        return Ok(AntigravitySettings::default());
    }

    let content = fs::read_to_string(&path).map_err(|e| {
        AppError::io(&path, e)
    })?;

    serde_json::from_str(&content).map_err(|e| {
        AppError::json(&path, e)
    })
}

/// 将设置写回 Antigravity CLI 主设置文件（原子写）
pub fn write_antigravity_settings(settings: &AntigravitySettings) -> Result<(), AppError> {
    let path = get_antigravity_settings_path();

    // 确保目录存在
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            AppError::io(parent, e)
        })?;
    }

    let content = serde_json::to_string_pretty(settings).map_err(|e| {
        AppError::json(&path, e)
    })?;

    write_text_file(&path, &content)
}

// ─────────────────────────────────────────────
// mcp_config.json
// ─────────────────────────────────────────────

/// 单个 MCP Server 的配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntigravityMcpServer {
    pub command: String,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,

    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub env: HashMap<String, String>,

    /// 超时时间（毫秒）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
}

/// Antigravity CLI `mcp_config.json` 结构
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AntigravityMcpConfig {
    #[serde(rename = "mcpServers", default)]
    pub mcp_servers: HashMap<String, AntigravityMcpServer>,
}

/// 读取 Antigravity CLI MCP 配置
pub fn read_antigravity_mcp_config() -> Result<AntigravityMcpConfig, AppError> {
    let path = get_antigravity_mcp_config_path();

    if !path.exists() {
        log::info!("Antigravity mcp_config.json 不存在，返回空配置");
        return Ok(AntigravityMcpConfig::default());
    }

    let content = fs::read_to_string(&path).map_err(|e| {
        AppError::io(&path, e)
    })?;

    serde_json::from_str(&content).map_err(|e| {
        AppError::json(&path, e)
    })
}

/// 将 MCP 配置写回 Antigravity CLI（原子写）
pub fn write_antigravity_mcp_config(config: &AntigravityMcpConfig) -> Result<(), AppError> {
    let path = get_antigravity_mcp_config_path();

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            AppError::io(parent, e)
        })?;
    }

    let content = serde_json::to_string_pretty(config).map_err(|e| {
        AppError::json(&path, e)
    })?;

    write_text_file(&path, &content)
}

// ─────────────────────────────────────────────
// 单元测试
// ─────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn antigravity_settings_roundtrip() {
        let settings = AntigravitySettings {
            model: Some("gemini-2.5-pro".to_string()),
            color_scheme: Some("dark".to_string()),
            trusted_workspaces: vec!["/home/user/project".to_string()],
            base_url: None,
            env_key: None,
            extra: HashMap::new(),
        };

        let json = serde_json::to_string(&settings).unwrap();
        let parsed: AntigravitySettings = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.model, settings.model);
        assert_eq!(parsed.color_scheme, settings.color_scheme);
        assert_eq!(parsed.trusted_workspaces, settings.trusted_workspaces);
    }

    #[test]
    fn antigravity_settings_preserves_extra_fields() {
        let json = r#"{"model":"gemini-2.0","colorScheme":"dark","trustedWorkspaces":[],"unknownField":42}"#;
        let settings: AntigravitySettings = serde_json::from_str(json).unwrap();
        let output = serde_json::to_string(&settings).unwrap();

        // The unknown field must survive the roundtrip
        assert!(output.contains("unknownField"));
    }

    #[test]
    fn antigravity_mcp_config_roundtrip() {
        let mut servers = HashMap::new();
        servers.insert(
            "time".to_string(),
            AntigravityMcpServer {
                command: "npx".to_string(),
                args: vec!["-y".to_string(), "@modelcontextprotocol/server-time".to_string()],
                env: HashMap::new(),
                timeout: Some(60000),
            },
        );

        let config = AntigravityMcpConfig { mcp_servers: servers };
        let json = serde_json::to_string(&config).unwrap();
        let parsed: AntigravityMcpConfig = serde_json::from_str(&json).unwrap();

        assert!(parsed.mcp_servers.contains_key("time"));
    }
}
