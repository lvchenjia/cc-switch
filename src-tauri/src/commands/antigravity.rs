use crate::antigravity_config::{
    self, AntigravityMcpConfig, AntigravitySettings,
};

/// 获取 Antigravity CLI 的 settings.json 设置
#[tauri::command]
pub fn get_antigravity_settings() -> Result<AntigravitySettings, String> {
    antigravity_config::read_antigravity_settings().map_err(|e| e.to_string())
}

/// 保存 Antigravity CLI 的 settings.json 设置
#[tauri::command]
pub fn set_antigravity_settings(settings: AntigravitySettings) -> Result<(), String> {
    antigravity_config::write_antigravity_settings(&settings).map_err(|e| e.to_string())
}

/// 获取 Antigravity CLI 的 mcp_config.json 配置
#[tauri::command]
pub fn get_antigravity_mcp_config() -> Result<AntigravityMcpConfig, String> {
    antigravity_config::read_antigravity_mcp_config().map_err(|e| e.to_string())
}

/// 保存 Antigravity CLI 的 mcp_config.json 配置
#[tauri::command]
pub fn set_antigravity_mcp_config(config: AntigravityMcpConfig) -> Result<(), String> {
    antigravity_config::write_antigravity_mcp_config(&config).map_err(|e| e.to_string())
}
