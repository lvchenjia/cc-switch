//! Antigravity MCP 同步和导入模块

use serde_json::Value;
use std::collections::HashMap;

use crate::app_config::{McpApps, McpServer, MultiAppConfig};
use crate::antigravity_config::{
    read_antigravity_mcp_config, write_antigravity_mcp_config,
    get_antigravity_dir, AntigravityMcpServer,
};
use crate::error::AppError;

use super::validation::{extract_server_spec, validate_server_spec};

fn should_sync_antigravity_mcp() -> bool {
    // Antigravity 未安装/未初始化时跳过
    get_antigravity_dir().exists()
}

/// 将 Value 规范转换为 AntigravityMcpServer
fn value_to_antigravity_server(id: &str, spec: &Value) -> Result<AntigravityMcpServer, AppError> {
    let mut obj = if let Some(map) = spec.as_object() {
        map.clone()
    } else {
        return Err(AppError::McpValidation(format!(
            "MCP 服务器 '{id}' 不是对象"
        )));
    };

    // 如果有 server 字段，解包它
    if let Some(server_val) = obj.remove("server") {
        if let Some(server_obj) = server_val.as_object() {
            obj = server_obj.clone();
        }
    }

    let command = obj
        .get("command")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::McpValidation(format!("MCP 服务器 '{id}' 缺少 command 字段")))?
        .to_string();

    let args = obj
        .get("args")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let env = obj
        .get("env")
        .and_then(|v| v.as_object())
        .map(|map| {
            map.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();

    // 提取 timeout 毫秒数
    const DEFAULT_STARTUP_MS: u64 = 10_000;
    const DEFAULT_TOOL_MS: u64 = 60_000;

    let extract_timeout = |obj: &mut serde_json::Map<String, Value>, key: &str, multiplier: u64| -> Option<u64> {
        obj.remove(key).and_then(|val| {
            val.as_u64()
                .map(|n| n * multiplier)
                .or_else(|| val.as_f64().map(|f| (f * multiplier as f64) as u64))
        })
    };

    let startup_ms = extract_timeout(&mut obj, "startup_timeout_sec", 1000)
        .or_else(|| extract_timeout(&mut obj, "startup_timeout_ms", 1))
        .unwrap_or(DEFAULT_STARTUP_MS);
    let tool_ms = extract_timeout(&mut obj, "tool_timeout_sec", 1000)
        .or_else(|| extract_timeout(&mut obj, "tool_timeout_ms", 1))
        .unwrap_or(DEFAULT_TOOL_MS);

    let final_timeout = startup_ms.max(tool_ms);

    Ok(AntigravityMcpServer {
        command,
        args,
        env,
        timeout: Some(final_timeout),
    })
}

/// 将 AntigravityMcpServer 转换为统一 Value 规范
fn antigravity_server_to_value(server: &AntigravityMcpServer) -> Value {
    serde_json::json!({
        "type": "stdio",
        "command": server.command,
        "args": server.args,
        "env": server.env,
        "startup_timeout_ms": server.timeout.unwrap_or(10_000),
    })
}

/// 将 config.json 中 Antigravity 的 enabled==true 项写入 Antigravity MCP 配置
#[allow(dead_code)]
pub fn sync_enabled_to_antigravity(config: &MultiAppConfig) -> Result<(), AppError> {
    if !should_sync_antigravity_mcp() {
        return Ok(());
    }

    let mut current = read_antigravity_mcp_config()?;
    current.mcp_servers.clear();

    for (id, entry) in config.mcp.gemini.servers.iter() {
        // 注：由于 Antigravity 在 McpRoot 中复用 gemini 字段作为 fallback，
        // 这里读取 config.mcp.gemini 是为了向后兼容和安全 fallback。
        let enabled = entry
            .get("enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !enabled {
            continue;
        }

        if let Ok(spec) = extract_server_spec(entry) {
            if let Ok(server) = value_to_antigravity_server(id, &spec) {
                current.mcp_servers.insert(id.clone(), server);
            }
        }
    }

    write_antigravity_mcp_config(&current)
}

/// 从 Antigravity MCP 配置导入到统一结构
pub fn import_from_antigravity(config: &mut MultiAppConfig) -> Result<usize, AppError> {
    let mcp_config = read_antigravity_mcp_config()?;
    if mcp_config.mcp_servers.is_empty() {
        return Ok(0);
    }

    let servers = config.mcp.servers.get_or_insert_with(HashMap::new);
    let mut changed = 0;
    let mut errors = Vec::new();

    for (id, server_data) in mcp_config.mcp_servers.iter() {
        let spec = antigravity_server_to_value(server_data);
        if let Err(e) = validate_server_spec(&spec) {
            log::warn!("跳过无效 MCP 服务器 '{id}': {e}");
            errors.push(format!("{id}: {e}"));
            continue;
        }

        if let Some(existing) = servers.get_mut(id) {
            if !existing.apps.antigravity {
                existing.apps.antigravity = true;
                changed += 1;
                log::info!("MCP 服务器 '{id}' 已启用 Antigravity 应用");
            }
        } else {
            servers.insert(
                id.clone(),
                McpServer {
                    id: id.clone(),
                    name: id.clone(),
                    server: spec,
                    apps: McpApps {
                        claude: false,
                        codex: false,
                        gemini: false,
                        opencode: false,
                        hermes: false,
                        antigravity: true,
                    },
                    description: None,
                    homepage: None,
                    docs: None,
                    tags: Vec::new(),
                },
            );
            changed += 1;
            log::info!("导入新 MCP 服务器 '{id}' 至 Antigravity");
        }
    }

    if !errors.is_empty() {
        log::warn!("导入完成，但有 {} 项失败: {:?}", errors.len(), errors);
    }

    Ok(changed)
}

/// 将单个 MCP 服务器同步到 Antigravity live 配置
pub fn sync_single_server_to_antigravity(
    _config: &MultiAppConfig,
    id: &str,
    server_spec: &Value,
) -> Result<(), AppError> {
    if !should_sync_antigravity_mcp() {
        return Ok(());
    }

    let mut current = read_antigravity_mcp_config()?;
    let server = value_to_antigravity_server(id, server_spec)?;
    current.mcp_servers.insert(id.to_string(), server);
    write_antigravity_mcp_config(&current)
}

/// 从 Antigravity live 配置中移除单个 MCP 服务器
pub fn remove_server_from_antigravity(id: &str) -> Result<(), AppError> {
    if !should_sync_antigravity_mcp() {
        return Ok(());
    }

    let mut current = read_antigravity_mcp_config()?;
    if current.mcp_servers.remove(id).is_some() {
        write_antigravity_mcp_config(&current)?;
    }
    Ok(())
}
