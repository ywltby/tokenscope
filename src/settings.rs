//! 应用设置：`~/.tokenscope/settings.toml`（带注释、可手编的高级配置文件）。
//!
//! 迁移（2026-10-06）：原 `settings.json` 一次性导入——toml 缺失时读取同目录
//! 遗留 json（只读），`save()` 成功写出 toml 后将其改名 `settings.json.bak`
//! 保底（改名失败只延迟迁移，toml 在位后 load 不再读 json）。
//! 容忍策略：文件缺失/坏内容 → 报错或回退默认值；未知字段忽略，向前兼容。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// 关闭窗口按钮的默认动作（None = 每次询问；托盘「退出」不受此影响）。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CloseAction {
    /// 最小化到托盘（窗口隐藏，进程驻留）。
    Minimize,
    /// 直接退出进程。
    Quit,
}

/// 单一来源配置（C1）：稳定 ID = 结构字段（claude/codex），不引入字符串 ID。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SourceConfig {
    /// 停用后完全不采集该来源（统计与状态页均可见"已停用"）。
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 显式目录；None = 工具默认根（~/.claude/projects、~/.codex/sessions）。
    #[serde(default)]
    pub dir: Option<String>,
}

impl Default for SourceConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            dir: None,
        }
    }
}

/// 各来源的覆盖配置；None = 全默认（启用 + 默认目录）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct AgentSources {
    #[serde(default)]
    pub claude: Option<SourceConfig>,
    #[serde(default)]
    pub codex: Option<SourceConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    /// 定时自动同步在线价格（默认开启，每 24h）。
    #[serde(default = "default_true")]
    pub price_auto_sync: bool,
    /// C1：来源目录与启停配置（缺省 = 全启用 + 默认目录）。
    #[serde(default)]
    pub sources: AgentSources,
    /// 关闭窗口默认动作；None = 每次询问（前端弹窗，可勾选记忆）。
    #[serde(default)]
    pub close_action: Option<CloseAction>,
}

fn default_true() -> bool {
    true
}

impl Settings {
    /// 取某来源的有效配置（未配置 → 全默认）。
    pub fn source_config(&self, claude: bool) -> SourceConfig {
        let c = if claude {
            &self.sources.claude
        } else {
            &self.sources.codex
        };
        c.clone().unwrap_or_default()
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            price_auto_sync: true,
            sources: AgentSources::default(),
            close_action: None,
        }
    }
}

pub fn settings_path() -> Result<PathBuf> {
    Ok(crate::report::data_dir()?.join("settings.toml"))
}

/// 遗留设置文件（迁移前格式）：与 toml 同目录；路径相同（测试直传 json 路径）时
/// 不视为遗留文件。
fn legacy_json_path(path: &Path) -> Option<PathBuf> {
    let parent = path.parent()?;
    let legacy = parent.join("settings.json");
    (legacy != path).then_some(legacy)
}

/// 读设置：toml 缺失 → 尝试遗留 settings.json 一次性导入（只读，不落盘）；
/// 都缺失 → 默认值；坏内容 → Err（调用方警告并使用默认值/拒绝覆盖）。
pub fn load(path: &Path) -> Result<Settings> {
    if !path.exists() {
        if let Some(legacy) = legacy_json_path(path)
            && legacy.exists()
        {
            let text = std::fs::read_to_string(&legacy)
                .with_context(|| format!("读遗留设置失败: {}", legacy.display()))?;
            let s: Settings = serde_json::from_str(&text)
                .with_context(|| format!("遗留设置解析失败: {}", legacy.display()))?;
            log::info!("检测到遗留 settings.json，已导入其值（首次保存后迁移为 toml）");
            return Ok(s);
        }
        return Ok(Settings::default());
    }
    let text =
        std::fs::read_to_string(path).with_context(|| format!("读设置失败: {}", path.display()))?;
    let s: Settings =
        toml::from_str(&text).with_context(|| format!("设置解析失败: {}", path.display()))?;
    Ok(s)
}

/// 配置文件头注释：每次保存都随内容重写（GUI 保存会丢自定义注释，见头说明）。
pub const SETTINGS_HEADER: &str = "\
# TokenScope 设置文件（GUI 设置页与手工编辑共用；可直接编辑，保存后对下一次读取生效）
# 注意：GUI 保存会重写整个文件，自定义注释会丢失；字段说明见设置页「高级配置」。
";

/// 全字段注释模板：open_settings_file 首次创建用（全部键注释掉，解析即默认值）。
pub const SETTINGS_TEMPLATE: &str = "\
# TokenScope 设置文件（~/.tokenscope/settings.toml）
# ─────────────────────────────────────────────────────────
# 本文件是图形设置页的唯一事实源：页面上的修改会写回这里，
# 手动编辑保存后同样立即生效（每次读取都从磁盘加载）。
# 注意：GUI 保存时会重写整个文件，自定义注释会丢失（字段说明以本模板为准）。
#
# price_auto_sync = true | false
#     价格自动同步（默认开启，每 24h 检查一次；关闭后完全不联网）。
#
# close_action = \"minimize\" | \"quit\"
#     点击窗口关闭按钮的默认动作：minimize = 最小化到托盘；quit = 直接退出。
#     删除或注释此行 = 每次关闭弹窗询问（也可在设置页「关闭窗口时」修改）。
#
# [sources.claude] / [sources.codex]
#     enabled = true | false    停用后该来源完全不参与统计。
#     dir = \"D:/logs/claude\"    显式日志目录；省略 = 使用工具默认目录。
#     注意：两个启用的来源不能指向同一目录（保存时校验）。
";

/// Task 2：来源目录重叠校验——两个启用的 agent 指向同一规范化目录时
/// 拒绝保存（采集层仍有防御性去重兜底旧配置/符号链接）。
pub fn validate_no_overlap(
    claude_dir: Option<&str>,
    codex_dir: Option<&str>,
) -> Result<(), String> {
    let norm = |s: Option<&str>| -> Option<String> {
        s.filter(|d| !d.trim().is_empty()).map(|d| {
            crate::report::normalize_path(std::path::Path::new(d))
                .to_string_lossy()
                .to_lowercase()
        })
    };
    match (norm(claude_dir), norm(codex_dir)) {
        (Some(c), Some(x)) if c == x => Err(
            "Claude 与 Codex 来源目录指向同一位置，会导致重复统计；请为两者配置不同目录"
                .to_string(),
        ),
        _ => Ok(()),
    }
}

/// D3/R06：自动同步的允许判定——设置读取失败时返回 false（默认离线）。
/// 关闭自动同步的用户意图绝不能因设置损坏被悄悄重置成联网。
pub fn auto_sync_allowed(s: &Result<Settings, anyhow::Error>) -> bool {
    match s {
        Ok(s) => s.price_auto_sync,
        Err(_) => false,
    }
}

/// 确保设置文件以 toml 形态在位（open_settings_file 首建用）：
/// 已存在 → 原样不动；缺失但有遗留 json → load 导入 + save 迁移（值保留、
/// json 改名 .bak）；都缺失 → 写入全字段注释模板（解析即默认值）。
pub fn ensure_toml(path: &Path) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    let had_legacy = legacy_json_path(path).is_some_and(|p| p.exists());
    if had_legacy {
        let s = load(path)?;
        return save(path, &s);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("创建目录失败: {}", dir.display()))?;
    }
    crate::fsutil::atomic_write(path, SETTINGS_TEMPLATE.as_bytes())
        .with_context(|| format!("写设置模板失败: {}", path.display()))?;
    Ok(())
}

pub fn save(path: &Path, s: &Settings) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("创建目录失败: {}", dir.display()))?;
    }
    let body = format!("{SETTINGS_HEADER}{}", toml::to_string_pretty(s)?);
    // 先落 toml 再处理遗留文件：toml 在位后 load 即不再读 json（改名失败只是延迟迁移）。
    crate::fsutil::atomic_write(path, body.as_bytes())
        .with_context(|| format!("写设置失败: {}", path.display()))?;
    if let Some(legacy) = legacy_json_path(path)
        && legacy.exists()
    {
        let bak = legacy.with_extension("json.bak");
        match std::fs::rename(&legacy, &bak) {
            Ok(()) => log::info!("遗留 settings.json 已迁移，改名保留为 {}", bak.display()),
            Err(e) => log::warn!("遗留 settings.json 改名失败（不影响使用）: {e}"),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tokenscope-m11-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.join("settings.toml")
    }

    #[test]
    fn test_settings_roundtrip_with_close_action() {
        let path = tmp("roundtrip");
        let s = Settings {
            price_auto_sync: false,
            close_action: Some(CloseAction::Minimize),
            sources: crate::settings::AgentSources {
                claude: Some(SourceConfig {
                    enabled: false,
                    dir: Some("D:/logs/claude".into()),
                }),
                codex: None,
            },
        };
        save(&path, &s).unwrap();
        assert_eq!(load(&path).unwrap(), s);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("close_action = \"minimize\""),
            "关闭动作必须落盘: {text}"
        );
        // quit 同样往返；None 序列化为缺省（省略字段）
        let s2 = Settings {
            close_action: Some(CloseAction::Quit),
            ..s.clone()
        };
        save(&path, &s2).unwrap();
        assert_eq!(load(&path).unwrap(), s2);
        let s3 = Settings {
            close_action: None,
            ..s2.clone()
        };
        save(&path, &s3).unwrap();
        assert_eq!(load(&path).unwrap(), s3);
        assert!(
            !std::fs::read_to_string(&path)
                .unwrap()
                .contains("close_action")
        );
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn test_save_writes_comment_header_and_template_parses() {
        let path = tmp("header");
        save(&path, &Settings::default()).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.starts_with('#'),
            "配置文件必须以注释头开头（可手编自解释）"
        );
        assert!(text.contains("直接编辑"), "注释头必须说明可手编: {text}");
        // 模板全部键注释化：解析即默认值（open_settings_file 首建用）
        let parsed: Settings = toml::from_str(SETTINGS_TEMPLATE).unwrap();
        assert_eq!(parsed, Settings::default());
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn test_legacy_json_import() {
        // 迁移：settings.toml 缺失但遗留 settings.json 存在 → 导入其值；
        // load 保持只读（不落盘、不改名）。
        let dir = tmp("legacy-import");
        let dir = dir.parent().unwrap();
        std::fs::write(
            dir.join("settings.json"),
            r#"{"price_auto_sync": false, "sources": {"claude": {"enabled": true, "dir": "D:/x"}}}"#,
        )
        .unwrap();
        let toml_path = dir.join("settings.toml");
        let s = load(&toml_path).unwrap();
        assert!(!s.price_auto_sync);
        assert_eq!(
            s.sources.claude.as_ref().unwrap().dir.as_deref(),
            Some("D:/x")
        );
        assert!(s.close_action.is_none());
        assert!(!toml_path.exists(), "load 不得落盘");
        assert!(dir.join("settings.json").exists(), "load 不得改名遗留文件");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn test_legacy_json_renamed_to_bak_on_save() {
        // 迁移完成时点 = 首次成功保存：json 改名 .bak（内容保留），防"改了没生效"。
        let dir = tmp("legacy-bak");
        let dir = dir.parent().unwrap();
        std::fs::write(dir.join("settings.json"), r#"{"price_auto_sync": false}"#).unwrap();
        let toml_path = dir.join("settings.toml");
        let s = load(&toml_path).unwrap();
        save(&toml_path, &s).unwrap();
        assert!(toml_path.exists());
        assert!(
            !dir.join("settings.json").exists(),
            "迁移后遗留文件必须改名"
        );
        let bak = std::fs::read_to_string(dir.join("settings.json.bak")).unwrap();
        assert!(bak.contains("price_auto_sync"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn test_toml_takes_precedence_over_legacy() {
        // toml 在位后遗留 json 被忽略（即使改名失败也不回读旧值）。
        let dir = tmp("precedence");
        let dir = dir.parent().unwrap();
        std::fs::write(dir.join("settings.json"), r#"{"price_auto_sync": false}"#).unwrap();
        let toml_path = dir.join("settings.toml");
        std::fs::write(&toml_path, "price_auto_sync = true\n").unwrap();
        assert!(load(&toml_path).unwrap().price_auto_sync);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn test_legacy_json_bad_content_is_err() {
        // 坏 JSON：报错（绝不静默回默认后覆盖用户文件）；load 只读，文件原样保留。
        let dir = tmp("legacy-bad");
        let dir = dir.parent().unwrap();
        let corrupt = r#"{"price_auto_sync": true, "broken""#;
        std::fs::write(dir.join("settings.json"), corrupt).unwrap();
        assert!(load(&dir.join("settings.toml")).is_err());
        assert_eq!(
            std::fs::read_to_string(dir.join("settings.json")).unwrap(),
            corrupt
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn test_validate_no_overlap_parent_and_case_aliases() {
        // Task 2（审阅）：`..` 组件折叠 + 大小写不敏感——等价目录必须识别。
        // 注意：validate 不触达文件系统（保存前校验），走 fallback 规范化。
        assert!(validate_no_overlap(Some(r"a\b\..\shared"), Some(r"a\shared")).is_err());
        assert!(validate_no_overlap(Some("C:/Shared/Logs"), Some("c:/shared/logs")).is_err());
        assert!(validate_no_overlap(Some("a/shared/"), Some("a/shared")).is_err());
        // 不同目录不误报。
        assert!(validate_no_overlap(Some("a/one"), Some("a/two")).is_ok());
    }

    #[test]
    fn test_sync_disabled_on_invalid_settings() {
        // D3：设置损坏 → 自动同步按离线处理，不悄悄恢复联网。
        assert!(auto_sync_allowed(&Ok(Settings::default())));
        assert!(!auto_sync_allowed(&Err(anyhow::anyhow!("bad json"))));
        let off = Settings {
            price_auto_sync: false,
            ..Default::default()
        };
        assert!(!auto_sync_allowed(&Ok(off)));
    }

    #[test]
    fn test_source_config_defaults_and_lookup() {
        // C1（test_source_config_roundtrip 的默认侧）：未配置 → 启用 + 默认目录。
        let s = Settings::default();
        let c = s.source_config(true);
        assert!(c.enabled);
        assert!(c.dir.is_none());
        // 旧版 settings.json（无 sources 字段）→ 经遗留导入向前兼容。
        let dir = tmp("legacy-c1");
        let dir = dir.parent().unwrap();
        std::fs::write(dir.join("settings.json"), r#"{"price_auto_sync": true}"#).unwrap();
        let s = load(&dir.join("settings.toml")).unwrap();
        assert!(s.source_config(false).enabled);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn test_settings_missing_defaults() {
        assert_eq!(
            load(Path::new("Z:/no-such/settings.toml")).unwrap(),
            Settings::default()
        );
    }

    #[test]
    fn test_settings_corrupt_is_err() {
        let path = tmp("corrupt");
        std::fs::write(&path, "not [valid toml").unwrap();
        assert!(load(&path).is_err());
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn test_settings_unknown_fields_ignored() {
        let path = tmp("unknown");
        std::fs::write(&path, "price_auto_sync = false\nfuture_key = 123\n").unwrap();
        let s = load(&path).unwrap();
        assert!(!s.price_auto_sync);
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }
}
