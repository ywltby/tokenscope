//! PR3 S02：来源发现——把设置 + 启动环境解析为稳定排序的来源根集合。
//!
//! 职责边界（计划 §9.3）：
//! - **只做解析与探测**，不扫描用量、不打开历史库、不写任何文件；
//! - 固定候选 = 默认根 + 启动环境派生根 + 手动追加项；不枚举主目录、
//!   不提权、不做整盘搜索；
//! - 环境变量只来自 [`StartupEnv`] 快照（生产启动时捕获一次，每轮不得重读）；
//!   测试直接构造上下文，绝不读进程全局环境；
//! - 每轮重新规范化（不缓存根身份，§0 第 3 条）；同一规范化根合并 origin 标签；
//! - 行有效启用 = 应用总开关且至少一个 origin 启用；总开关不改写各行用户选择。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;

use crate::model::AgentKind;
use crate::settings::SourceConfig;

/// 探测分类（§9.3 第 8 条：缺失/无权限/普通文件/失效链接不合成"缺失"）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeState {
    /// 存在且是目录。
    Ready,
    /// 不存在（默认根缺失可隐藏；环境/手动根保留行）。
    Missing,
    /// 存在但读取被拒（权限等）——显示"无权限"，不伪报缺失。
    NoPermission(String),
    /// 存在但不是可用目录：普通文件或失效链接（detail 说明形态）。
    NotADirectory(String),
}

/// 一次根探测：分类 + 身份解析（可访问时 canonicalize，覆盖联接/符号链接/
/// 8.3 短名/大小写；不可访问时 `canonical = None`，身份未确认）。
#[derive(Debug, Clone)]
pub struct RootProbe {
    pub state: ProbeState,
    pub canonical: Option<PathBuf>,
}

/// 探测器注入点：生产用文件系统实现；测试注入全部分类，绝不触真实盘。
pub type ProbeFn = Arc<dyn Fn(&Path) -> RootProbe + Send + Sync>;

/// 生产探测器：`symlink_metadata` 分类 + `canonicalize` 身份。
pub fn filesystem_probe() -> ProbeFn {
    Arc::new(|path: &Path| {
        let link = match std::fs::symlink_metadata(path) {
            Ok(md) => md,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return RootProbe {
                    state: ProbeState::Missing,
                    canonical: None,
                };
            }
            Err(e) => {
                return RootProbe {
                    state: ProbeState::NoPermission(e.to_string()),
                    canonical: None,
                };
            }
        };
        match std::fs::metadata(path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if link.is_symlink() {
                    RootProbe {
                        state: ProbeState::NotADirectory("失效链接（目标不存在）".to_string()),
                        canonical: None,
                    }
                } else {
                    RootProbe {
                        state: ProbeState::Missing,
                        canonical: None,
                    }
                }
            }
            Err(e) => RootProbe {
                state: ProbeState::NoPermission(e.to_string()),
                canonical: None,
            },
            Ok(md) if !md.is_dir() => RootProbe {
                state: ProbeState::NotADirectory("路径存在但不是目录".to_string()),
                canonical: None,
            },
            Ok(_) => match path.canonicalize() {
                Ok(c) => RootProbe {
                    state: ProbeState::Ready,
                    canonical: Some(crate::report::strip_verbatim(c)),
                },
                Err(e) => RootProbe {
                    state: ProbeState::NoPermission(e.to_string()),
                    canonical: None,
                },
            },
        }
    })
}

/// 启动时捕获的环境快照（§2：只使用 TokenScope 启动时继承的值，修改后需重启）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StartupEnv {
    pub codex_home: Option<String>,
    pub claude_config_dir: Option<String>,
}

/// 生产启动路径调用一次（S04 接线）；采集轮次禁止调用。
pub fn capture_startup_env() -> StartupEnv {
    StartupEnv {
        codex_home: std::env::var("CODEX_HOME").ok(),
        claude_config_dir: std::env::var("CLAUDE_CONFIG_DIR").ok(),
    }
}

/// 来源候选的出处（决定启停控制、标签与映射绑定）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OriginKind {
    /// 应用默认根。
    Default,
    /// 环境变量派生根（值 = 变量名）。
    Env(&'static str),
    /// 手动追加。
    Manual,
}

impl OriginKind {
    /// 标签（设置页"来源为默认自动/环境/手动多标签"用）。
    pub fn label(&self) -> &'static str {
        match self {
            OriginKind::Default => "默认自动",
            OriginKind::Env("CODEX_HOME") => "环境 CODEX_HOME",
            OriginKind::Env("CLAUDE_CONFIG_DIR") => "环境 CLAUDE_CONFIG_DIR",
            OriginKind::Env(_) => "环境",
            OriginKind::Manual => "手动",
        }
    }
    /// 稳定排序键：默认 < 环境 < 手动。
    fn order(&self) -> u8 {
        match self {
            OriginKind::Default => 0,
            OriginKind::Env(_) => 1,
            OriginKind::Manual => 2,
        }
    }
}

/// 一个 origin：该根从哪个候选来，带自己的配置路径与启用值。
#[derive(Debug, Clone)]
pub struct RootOrigin {
    pub kind: OriginKind,
    /// 配置路径（排除项匹配用稳定绝对配置路径；手动 = 追加项原样路径）。
    pub config_path: String,
    pub enabled: bool,
}

/// 解析后的一个来源根（同一规范化身份的 origin 已合并）。
#[derive(Debug, Clone)]
pub struct ResolvedRoot {
    pub agent: AgentKind,
    /// 显示路径（origin 稳定序里第一个的配置路径）。
    pub display_path: String,
    /// 规范化比较键（canonical 优先；不可访问用逻辑折叠）。
    pub identity_key: String,
    /// 身份是否已确认（canonicalize 成功；未确认时显示"身份未确认"诊断）。
    pub identity_confirmed: bool,
    pub state: ProbeState,
    pub origins: Vec<RootOrigin>,
    /// 项目映射绑定（仅 Claude；None = 禁用映射，诊断说明原因）。
    pub mapping_path: Option<PathBuf>,
    pub diagnostics: Vec<String>,
    /// 有效启停 = 应用总开关且至少一个 origin 启用。
    pub effective_enabled: bool,
}

impl ResolvedRoot {
    /// 行 id（§9.3 第 5 条：不用数组下标；规范化身份键跨轮稳定）。
    pub fn id(&self) -> &str {
        &self.identity_key
    }
    /// 是否为"全部 origin 都是默认且缺失"的行（设置列表可隐藏；仍参与新增校验）。
    pub fn hidden_by_default_missing(&self) -> bool {
        matches!(self.state, ProbeState::Missing)
            && self.origins.iter().all(|o| o.kind == OriginKind::Default)
    }
}

/// 一次来源解析的结果。
#[derive(Debug, Clone)]
pub struct ResolvedSources {
    /// 稳定排序（agent、identity_key）。
    pub roots: Vec<ResolvedRoot>,
    /// 上下文签名：根集合、映射、启停与探测状态的稳定摘要（采集复用键用；
    /// 不含随机值/当前时间，热轮不因此失效）。
    pub signature: String,
}

/// 发现上下文：注入默认锚点、启动环境与探测器（§9.3 第 1、2 条）。
#[derive(Clone)]
pub struct DiscoveryContext {
    /// 默认 Claude 日志根。
    pub claude_default_root: PathBuf,
    /// 默认 Claude 项目映射（A03；acceptance 模式为隔离根下的 claude.json）。
    pub claude_default_mapping: PathBuf,
    /// 默认 Codex 双根（sessions + archived_sessions）。
    pub codex_default_roots: Vec<PathBuf>,
    pub env: StartupEnv,
    pub probe: ProbeFn,
}

impl DiscoveryContext {
    /// 生产上下文：默认锚点与采集侧同源（acceptance 构建自动落到隔离根），
    /// 环境用启动快照——本函数不在采集轮次调用。
    pub fn production(env: StartupEnv) -> Result<Self> {
        if let Some(claude_src) = crate::acceptance::source_dir_override(true) {
            let codex_src = crate::acceptance::source_dir_override(false)
                .ok_or_else(|| anyhow::anyhow!("验收根缺少 codex 来源目录"))?;
            return Ok(Self {
                claude_default_root: claude_src.join("projects"),
                claude_default_mapping: claude_src.join("claude.json"),
                codex_default_roots: vec![
                    codex_src.join("sessions"),
                    codex_src.join("archived_sessions"),
                ],
                env,
                probe: filesystem_probe(),
            });
        }
        let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("无法定位用户主目录"))?;
        Ok(Self::for_test(home, env, filesystem_probe()))
    }

    /// 测试上下文：注入主目录（默认锚点由它派生），与真实布局一致。
    pub fn for_test(home: impl Into<PathBuf>, env: StartupEnv, probe: ProbeFn) -> Self {
        let home = home.into();
        Self {
            claude_default_root: home.join(".claude").join("projects"),
            claude_default_mapping: home.join(".claude.json"),
            codex_default_roots: vec![
                home.join(".codex").join("sessions"),
                home.join(".codex").join("archived_sessions"),
            ],
            env,
            probe,
        }
    }

    fn env_candidate(&self, raw: Option<&String>) -> Option<(PathBuf, Vec<String>)> {
        let raw = raw?;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Some((PathBuf::new(), vec![])); // 空串：调用方按"忽略"处理
        }
        let p = Path::new(trimmed);
        if !p.is_absolute() {
            return Some((
                PathBuf::new(),
                vec![format!(
                    "环境变量指向相对路径 {trimmed:?}，已忽略（环境变量必须是绝对路径；修改后需重启）"
                )],
            ));
        }
        Some((p.to_path_buf(), vec![]))
    }
}

/// 统一的 canonical 身份（canonicalize + 剥离 Windows `\?\` 前缀）；
/// 生产探测器与测试注入共用，保证行 id 形态一致。
pub fn canonical_identity(path: &Path) -> Option<PathBuf> {
    path.canonicalize().ok().map(crate::report::strip_verbatim)
}

/// 逻辑规范化键：组件折叠 + Windows 大小写折叠（与设置侧排除项匹配同一函数）。
fn logical_key(p: &Path) -> String {
    crate::settings::normalized_key(p)
}

struct Candidate {
    agent: AgentKind,
    kind: OriginKind,
    config_path: String,
    enabled: bool,
    probe: RootProbe,
    /// 已核实的映射绑定（默认 Claude 根 = A03 默认绑定；其余为 None）。
    mapping: Option<PathBuf>,
    /// 映射**明确禁用**（自定义/手动/环境根，A03）——区别于"未指定"，
    /// 不得被合并阶段的默认修复逻辑改成默认绑定。
    mapping_disabled: bool,
    diagnostics: Vec<String>,
}

/// 解析来源根集合（§2：每轮采集开始时调用并冻结结果）。
///
/// 未迁移旧格式（[`SourceConfig::is_legacy_dir_semantics`]）仍按旧语义：
/// 显式 dir 单根取代默认根（映射禁用），无 dir 时仅默认根，Codex 环境根
/// **替代**默认根——迁移失败窗口期不扩大读取范围；新格式为追加语义。
pub fn resolve_sources(
    ctx: &DiscoveryContext,
    claude: &SourceConfig,
    codex: &SourceConfig,
) -> ResolvedSources {
    let mut candidates: Vec<Candidate> = Vec::new();
    // ── Claude ──
    if claude.is_legacy_dir_semantics() {
        let root = claude
            .dir
            .as_deref()
            .filter(|d| !d.trim().is_empty())
            .map_or_else(|| ctx.claude_default_root.clone(), PathBuf::from);
        let mapping = if claude.dir.is_some() {
            None // A03：自定义根不读取默认账户映射
        } else {
            Some(ctx.claude_default_mapping.clone())
        };
        candidates.push(Candidate {
            agent: AgentKind::ClaudeCode,
            kind: OriginKind::Default,
            config_path: root.display().to_string(),
            enabled: true,
            probe: (ctx.probe)(&root),
            mapping,
            mapping_disabled: claude.dir.is_some(),
            diagnostics: vec!["旧格式配置尚未迁移：仍按 v1 语义解析根".to_string()],
        });
    } else {
        let mut claude_default_diag = Vec::new();
        if let Some((root, mut env_diag)) = ctx.env_candidate(ctx.env.claude_config_dir.as_ref()) {
            if root.as_os_str().is_empty() {
                env_diag.push("环境变量 CLAUDE_CONFIG_DIR 为空串，已忽略".to_string());
            } else {
                let env_root = root.join("projects");
                candidates.push(Candidate {
                    agent: AgentKind::ClaudeCode,
                    kind: OriginKind::Env("CLAUDE_CONFIG_DIR"),
                    config_path: env_root.display().to_string(),
                    enabled: claude.auto_discover && !excluded(claude, &env_root),
                    probe: (ctx.probe)(&env_root),
                    // 映射位置未核实 → 明确降级：禁用映射（§2 结论），不借用默认绑定
                    mapping: None,
                    mapping_disabled: true,
                    diagnostics: vec![],
                });
            }
            claude_default_diag.append(&mut env_diag);
        }
        candidates.push(Candidate {
            agent: AgentKind::ClaudeCode,
            kind: OriginKind::Default,
            config_path: ctx.claude_default_root.display().to_string(),
            enabled: claude.auto_discover && !excluded(claude, &ctx.claude_default_root),
            probe: (ctx.probe)(&ctx.claude_default_root),
            mapping: Some(ctx.claude_default_mapping.clone()),
            mapping_disabled: false,
            diagnostics: claude_default_diag,
        });
        for entry in &claude.additional_paths {
            let root = PathBuf::from(&entry.path);
            candidates.push(Candidate {
                agent: AgentKind::ClaudeCode,
                kind: OriginKind::Manual,
                config_path: entry.path.clone(),
                enabled: entry.enabled,
                probe: (ctx.probe)(&root),
                mapping: None, // 手动根不猜映射（A03）
                mapping_disabled: true,
                diagnostics: vec![],
            });
        }
    }
    // ── Codex ──
    if codex.is_legacy_dir_semantics() {
        let explicit = codex
            .dir
            .as_deref()
            .filter(|d| !d.trim().is_empty())
            .map(PathBuf::from);
        let roots: Vec<PathBuf> = match explicit {
            Some(d) => vec![d],
            None => match ctx.env_candidate(ctx.env.codex_home.as_ref()) {
                // 旧语义：CODEX_HOME 替代默认 .codex
                Some((root, _)) if !root.as_os_str().is_empty() => {
                    vec![root.join("sessions"), root.join("archived_sessions")]
                }
                _ => ctx.codex_default_roots.clone(),
            },
        };
        for root in roots {
            candidates.push(Candidate {
                agent: AgentKind::Codex,
                kind: OriginKind::Default,
                config_path: root.display().to_string(),
                enabled: true,
                probe: (ctx.probe)(&root),
                mapping: None,
                mapping_disabled: false,
                diagnostics: vec!["旧格式配置尚未迁移：仍按 v1 语义解析根".to_string()],
            });
        }
    } else {
        let mut codex_default_diag = Vec::new();
        if let Some((base, mut env_diag)) = ctx.env_candidate(ctx.env.codex_home.as_ref()) {
            if base.as_os_str().is_empty() {
                env_diag.push("环境变量 CODEX_HOME 为空串，已忽略".to_string());
            } else {
                for root in [base.join("sessions"), base.join("archived_sessions")] {
                    candidates.push(Candidate {
                        agent: AgentKind::Codex,
                        kind: OriginKind::Env("CODEX_HOME"),
                        config_path: root.display().to_string(),
                        enabled: codex.auto_discover && !excluded(codex, &root),
                        probe: (ctx.probe)(&root),
                        mapping: None,
                        mapping_disabled: true,
                        diagnostics: vec![],
                    });
                }
            }
            codex_default_diag.append(&mut env_diag);
        }
        for root in &ctx.codex_default_roots {
            candidates.push(Candidate {
                agent: AgentKind::Codex,
                kind: OriginKind::Default,
                config_path: root.display().to_string(),
                enabled: codex.auto_discover && !excluded(codex, root),
                probe: (ctx.probe)(root),
                mapping: None,
                mapping_disabled: false,
                diagnostics: codex_default_diag.clone(),
            });
        }
        for entry in &codex.additional_paths {
            let root = PathBuf::from(&entry.path);
            candidates.push(Candidate {
                agent: AgentKind::Codex,
                kind: OriginKind::Manual,
                config_path: entry.path.clone(),
                enabled: entry.enabled,
                probe: (ctx.probe)(&root),
                mapping: None,
                mapping_disabled: true,
                diagnostics: vec![],
            });
        }
    }
    let mut resolved = merge_candidates(candidates);
    // 行有效启用 = 应用总开关且至少一个 origin 启用（§9.3 第 3 条）；
    // 总开关只作用于有效值，不改写各行用户选择（重新开启时恢复原选择）。
    for row in &mut resolved.roots {
        let app_enabled = if row.agent == AgentKind::ClaudeCode {
            claude.enabled
        } else {
            codex.enabled
        };
        row.effective_enabled = row.effective_enabled && app_enabled;
    }
    resolved
}

fn excluded(cfg: &SourceConfig, path: &Path) -> bool {
    let key = logical_key(path);
    cfg.excluded_auto_paths
        .iter()
        .any(|p| logical_key(Path::new(p)) == key)
}

/// 合并已核实绑定：任一候选给出默认绑定即保留（§2：冲突不猜测、优先已核实）。
fn merge_verified(a: Option<PathBuf>, b: Option<PathBuf>) -> Option<PathBuf> {
    a.or(b)
}

/// 同一规范化根合并 origin 标签；绑定已核实默认映射；产出诊断与稳定签名。
fn merge_candidates(candidates: Vec<Candidate>) -> ResolvedSources {
    let mut merged: Vec<ResolvedRoot> = Vec::new();
    let mut verified: Vec<Option<PathBuf>> = Vec::new();
    let mut disabled: Vec<bool> = Vec::new();
    for candidate in candidates {
        let identity = candidate
            .probe
            .canonical
            .clone()
            .map(|c| c.display().to_string())
            .unwrap_or_else(|| logical_key(Path::new(&candidate.config_path)));
        let confirmed = candidate.probe.canonical.is_some();
        if let Some(idx) = merged
            .iter()
            .position(|r| r.agent == candidate.agent && r.identity_key == identity)
        {
            // 同根：合并标签，保留已核实的默认绑定（§2：冲突不猜测）
            if candidate.mapping.is_some() && merged[idx].mapping_path.is_none() {
                merged[idx].mapping_path = candidate.mapping.clone();
            }
            verified[idx] = merge_verified(verified[idx].clone(), candidate.mapping.clone());
            disabled[idx] |= candidate.mapping_disabled;
            merged[idx].origins.push(RootOrigin {
                kind: candidate.kind,
                config_path: candidate.config_path,
                enabled: candidate.enabled,
            });
            merged[idx].diagnostics.extend(candidate.diagnostics);
            if merged[idx].state != candidate.probe.state {
                // 同一物理根两次探测结论不一致（不应发生）：记录，不猜测
                let prev = merged[idx].state.clone();
                merged[idx].diagnostics.push(format!(
                    "同一根在不同候选下探测状态不同：{:?} / {:?}",
                    prev, candidate.probe.state
                ));
            }
        } else {
            merged.push(ResolvedRoot {
                agent: candidate.agent,
                display_path: candidate.config_path.clone(),
                identity_key: identity,
                identity_confirmed: confirmed,
                state: candidate.probe.state,
                origins: vec![RootOrigin {
                    kind: candidate.kind,
                    config_path: candidate.config_path,
                    enabled: candidate.enabled,
                }],
                mapping_path: candidate.mapping.clone(),
                diagnostics: candidate.diagnostics,
                effective_enabled: false,
            });
            verified.push(candidate.mapping.clone());
            disabled.push(candidate.mapping_disabled);
        }
    }
    for (idx, row) in merged.iter_mut().enumerate() {
        row.origins.sort_by(|a, b| {
            a.kind
                .order()
                .cmp(&b.kind.order())
                .then_with(|| a.config_path.cmp(&b.config_path))
        });
        row.effective_enabled = row.origins.iter().any(|o| o.enabled);
        if !row.identity_confirmed {
            row.diagnostics
                .push("身份未确认：目录不可访问，仅按逻辑路径比较".to_string());
        }
        if row.agent == AgentKind::ClaudeCode {
            // 映射裁决：已核实绑定优先（默认根/legacy 无 dir）；明确禁用（A03：
            // 自定义/手动/环境根）不得被默认修复改成默认绑定
            if let Some(path) = &verified[idx] {
                row.mapping_path = Some(path.clone());
                if row.origins.len() > 1 {
                    row.diagnostics
                        .push("同一物理根由多个候选指到：项目映射采用已核实的默认绑定".to_string());
                }
            } else if disabled[idx] && row.mapping_path.is_none() {
                let env_root = row
                    .origins
                    .iter()
                    .any(|o| o.kind == OriginKind::Env("CLAUDE_CONFIG_DIR"));
                row.diagnostics.push(if env_root {
                    "CLAUDE_CONFIG_DIR 根的项目映射位置未核实，已禁用映射（不借用默认映射）"
                        .to_string()
                } else {
                    "该根为手动追加目录，项目映射已禁用（沿用 A03）".to_string()
                });
            } else if row.mapping_path.is_none() {
                // 未指定（理论不可达）：保守禁用，不猜
                row.diagnostics
                    .push("项目映射绑定未指定，已禁用".to_string());
            }
        }
    }
    merged.sort_by(|a, b| {
        a.agent
            .as_str()
            .cmp(b.agent.as_str())
            .then_with(|| a.identity_key.cmp(&b.identity_key))
    });
    let signature = context_signature(&merged);
    ResolvedSources {
        roots: merged,
        signature,
    }
}

/// FNV-1a 64：无依赖、跨轮稳定的上下文签名。
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in bytes {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn context_signature(roots: &[ResolvedRoot]) -> String {
    let mut material = String::new();
    for row in roots {
        material.push_str(&format!(
            "{}|{}|{}|{:?}|map={}|origins={};",
            row.agent.as_str(),
            row.identity_key,
            row.effective_enabled,
            row.state,
            row.mapping_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            row.origins
                .iter()
                .map(|o| format!("{:?}={}={}", o.kind, o.config_path, o.enabled))
                .collect::<Vec<_>>()
                .join(","),
        ));
    }
    format!("{:016x}", fnv1a(material.as_bytes()))
}

// ── 行操作（§9.3 第 4、5 条）：启停作用于该行全部 origin ─────────────

/// 目标行的配置路径集合（调用方从解析行提取；配置字符串与落盘逐字对应，
/// 避免 canonical 身份与逻辑键两种形态之间的字符串匹配错位）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RowConfigPaths {
    /// 该行自动 origin（默认/环境）的配置路径。
    pub auto: Vec<String>,
    /// 该行手动 origin 的配置路径。
    pub manual: Vec<String>,
}

/// 行启停：禁用 = 自动 origin 的配置路径加入排除项 + 该行手动项
/// enabled=false；开启 = 逆操作。总开关（`SourceConfig::enabled`）不在此
/// 改写。两个集合都为空 → 明确错误（调用方应先用 `row_exists` 定位行）。
pub fn set_row_enabled(
    cfg: &mut SourceConfig,
    paths: &RowConfigPaths,
    enabled: bool,
) -> Result<(), String> {
    let mut manual_touched = false;
    for entry in &mut cfg.additional_paths {
        if paths.manual.contains(&entry.path) {
            entry.enabled = enabled;
            manual_touched = true;
        }
    }
    if enabled {
        let row_keys: Vec<String> = paths
            .auto
            .iter()
            .map(|p| logical_key(Path::new(p)))
            .collect();
        cfg.excluded_auto_paths
            .retain(|p| !row_keys.contains(&logical_key(Path::new(p))));
    } else {
        for path in &paths.auto {
            if !cfg
                .excluded_auto_paths
                .iter()
                .any(|p| logical_key(Path::new(p)) == logical_key(Path::new(path)))
            {
                cfg.excluded_auto_paths.push(path.clone());
            }
        }
    }
    if paths.auto.is_empty() && !manual_touched {
        return Err("该行没有可操作的配置项（来源可能已变化，请重试）".to_string());
    }
    Ok(())
}

/// 移除仅删除该行手动项；自动标签保持原启停（§9.3 第 4 条）。
/// 行内没有手动 origin → 明确错误。
pub fn remove_manual_from_row(
    cfg: &mut SourceConfig,
    paths: &RowConfigPaths,
) -> Result<(), String> {
    let before = cfg.additional_paths.len();
    cfg.additional_paths
        .retain(|e| !paths.manual.contains(&e.path));
    if cfg.additional_paths.len() < before {
        return Ok(());
    }
    // 该行没有手动 origin（纯自动行）→ 不可移除
    Err("该行没有可移除的手动目录".to_string())
}

/// 供写接口按 id 定位行是否仍然存在（不存在 → 提示"来源已变化，请重试"）。
pub fn row_exists(roots: &[ResolvedRoot], agent: AgentKind, id: &str) -> bool {
    roots
        .iter()
        .any(|r| r.agent == agent && r.identity_key == id)
}
