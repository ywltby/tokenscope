//! PR3 S02（计划 §6/§9.3）：固定候选与环境发现的解析契约。
//!
//! 全部用例注入主目录、启动环境与探测器，不读真实主目录与环境变量（§8）。
//! 覆盖：默认/环境/手动三类候选、环境变量空值/相对/缺失语义、同根别名合并
//! 标签、映射绑定（A03 + CLAUDE_CONFIG_DIR 降级）、总开关与行开关组合、
//! 排除项、行启停→重解析→移除→开启循环、未知 id 拒绝、新目录下一轮出现、
//! 每轮重新规范化与上下文签名稳定性。
//!
//! 边界：本文件只做解析（不扫描用量）；真实文件系统 fixture 只验证
//! "新目录下一轮出现"，联接环与多根采集在 S03 的 `source_paths_collection`。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tokenscope::model::AgentKind;
use tokenscope::settings::SourceConfig;
use tokenscope::source::discovery::{
    DiscoveryContext, OriginKind, ProbeFn, ProbeState, RootProbe, StartupEnv, filesystem_probe,
    remove_manual_from_row, resolve_sources, row_exists, set_row_enabled,
};

static SEQ: AtomicU64 = AtomicU64::new(0);

fn temp_dir(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "tokenscope-sp-discovery-{tag}-{}-{n}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// 假身份规范化：小写 + 分隔符折叠 + 去尾分隔符——跨平台确定性地模拟
/// canonicalize 的"同根别名"（`join("a/b")` 与 `join("a").join("b")` 的
/// 字符串分隔符不同，身份必须折叠）。
fn norm(text: &str) -> String {
    let mut s = text
        .trim_end_matches(['/', '\\'])
        .replace('/', "\\")
        .to_lowercase();
    if let Some(stripped) = s.strip_prefix(r"\\?\") {
        s = stripped.to_string();
    }
    s
}

fn fake_canon(path: &Path) -> PathBuf {
    PathBuf::from(norm(&path.to_string_lossy()))
}

/// 表驱动探测器：命中表项用给定结论，未命中默认 Ready（假身份）。
fn table_probe(table: Vec<(PathBuf, RootProbe)>) -> ProbeFn {
    let map: Arc<HashMap<String, RootProbe>> = Arc::new(
        table
            .into_iter()
            .map(|(k, v)| (norm(&k.to_string_lossy()), v))
            .collect(),
    );
    Arc::new(move |path: &Path| {
        let key = norm(&path.to_string_lossy());
        if let Some(probe) = map.get(&key) {
            return probe.clone();
        }
        RootProbe {
            state: ProbeState::Ready,
            canonical: Some(fake_canon(path)),
        }
    })
}

fn ctx_for(home: &Path, env: StartupEnv, probe: ProbeFn) -> DiscoveryContext {
    DiscoveryContext::for_test(home, env, probe)
}

fn default_claude() -> SourceConfig {
    SourceConfig::default()
}

fn default_codex() -> SourceConfig {
    SourceConfig::default()
}

// ── 默认候选、状态分类与映射绑定 ─────────────────────────────────

/// 默认三根（Claude 单根 + Codex 双根）带正确 origin、映射与启用值。
#[test]
fn default_roots_resolve_with_origins_and_mapping() {
    let home = temp_dir("defaults");
    std::fs::create_dir_all(home.join(".claude/projects")).unwrap();
    std::fs::create_dir_all(home.join(".codex/sessions")).unwrap();
    let ctx = ctx_for(&home, StartupEnv::default(), filesystem_probe());
    let out = resolve_sources(
        &ctx,
        &default_claude(),
        &default_codex(),
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    // WB04 产品口径变化：默认候选集从 3 根扩为 5 根
    //（Claude 1 + Codex 2 + WorkBuddy 2；两个 WorkBuddy 根在本用例主目录
    // 不存在，按 Missing 纯默认行保留）。
    assert_eq!(
        out.roots.len(),
        5,
        "Claude 1 根 + Codex 2 根 + WorkBuddy 2 根: {:?}",
        out.roots
    );
    let claude = out
        .roots
        .iter()
        .find(|r| r.agent == AgentKind::ClaudeCode)
        .unwrap();
    assert_eq!(claude.origins.len(), 1);
    assert_eq!(claude.origins[0].kind, OriginKind::Default);
    assert!(claude.origins[0].enabled);
    assert!(claude.effective_enabled);
    assert!(claude.identity_confirmed);
    assert_eq!(claude.state, ProbeState::Ready);
    assert_eq!(
        claude.mapping_path.as_ref().unwrap(),
        &home.join(".claude.json"),
        "默认根绑定 A03 映射"
    );
    let codex: Vec<_> = out
        .roots
        .iter()
        .filter(|r| r.agent == AgentKind::Codex)
        .collect();
    assert_eq!(codex.len(), 2, "sessions 与 archived_sessions 是两行");
    assert!(codex.iter().all(|r| r.mapping_path.is_none()));
    assert!(codex.iter().any(|r| r.display_path.ends_with("sessions")));
    assert!(
        codex
            .iter()
            .any(|r| r.display_path.ends_with("archived_sessions"))
    );
    // 稳定排序：claude 在前，同 agent 按身份键
    assert_eq!(out.roots[0].agent, AgentKind::ClaudeCode);
}

/// 缺失 / 无权限 / 普通文件不合成同一种"缺失"（§9.3 第 8 条）。
#[test]
fn missing_permission_and_not_a_directory_are_distinct() {
    let home = temp_dir("states");
    let claude_root = home.join(".claude/projects");
    let sessions = home.join(".codex/sessions");
    let archived = home.join(".codex/archived_sessions");
    let probe = table_probe(vec![
        (
            claude_root.clone(),
            RootProbe {
                state: ProbeState::Missing,
                canonical: None,
            },
        ),
        (
            sessions.clone(),
            RootProbe {
                state: ProbeState::NoPermission("拒绝访问".to_string()),
                canonical: None,
            },
        ),
        (
            archived.clone(),
            RootProbe {
                state: ProbeState::NotADirectory("路径存在但不是目录".to_string()),
                canonical: None,
            },
        ),
    ]);
    let ctx = ctx_for(&home, StartupEnv::default(), probe);
    let out = resolve_sources(
        &ctx,
        &default_claude(),
        &default_codex(),
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    let claude = out
        .roots
        .iter()
        .find(|r| r.agent == AgentKind::ClaudeCode)
        .unwrap();
    assert_eq!(claude.state, ProbeState::Missing);
    let codex: Vec<_> = out
        .roots
        .iter()
        .filter(|r| r.agent == AgentKind::Codex)
        .collect();
    assert!(
        codex
            .iter()
            .any(|r| matches!(r.state, ProbeState::NoPermission(_)))
    );
    assert!(
        codex
            .iter()
            .any(|r| matches!(r.state, ProbeState::NotADirectory(_)))
    );
    // 不可访问 → 身份未确认诊断
    assert!(claude.diagnostics.iter().any(|d| d.contains("身份未确认")));
}

/// 环境变量三种形态：空串忽略（带诊断）、相对路径忽略并诊断、缺失绝对根保留行。
#[test]
fn env_variables_empty_relative_and_missing_are_handled() {
    let home = temp_dir("env-shapes");
    // 空串
    let ctx = ctx_for(
        &home,
        StartupEnv {
            codex_home: Some("  ".to_string()),
            claude_config_dir: None,
        },
        filesystem_probe(),
    );
    let out = resolve_sources(
        &ctx,
        &default_claude(),
        &default_codex(),
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    assert!(
        out.roots
            .iter()
            .all(|r| !matches!(r.origins[0].kind, OriginKind::Env(_))),
        "空串环境变量不产生环境根"
    );
    let codex_default = out
        .roots
        .iter()
        .find(|r| r.agent == AgentKind::Codex && r.display_path.ends_with("sessions"))
        .unwrap();
    assert!(
        codex_default
            .diagnostics
            .iter()
            .any(|d| d.contains("CODEX_HOME 为空串")),
        "{:?}",
        codex_default.diagnostics
    );
    // 相对路径
    let ctx = ctx_for(
        &home,
        StartupEnv {
            codex_home: Some("relative/logs".to_string()),
            claude_config_dir: Some("relative/claude".to_string()),
        },
        filesystem_probe(),
    );
    let out = resolve_sources(
        &ctx,
        &default_claude(),
        &default_codex(),
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    assert!(
        !out.roots.iter().any(|r| r
            .origins
            .iter()
            .any(|o| matches!(o.kind, OriginKind::Env(_)))),
        "相对路径环境变量不产生环境根"
    );
    assert!(
        out.roots
            .iter()
            .flat_map(|r| &r.diagnostics)
            .any(|d| d.contains("相对路径") && d.contains("已忽略")),
        "相对路径必须诊断: {:?}",
        out.roots
            .iter()
            .flat_map(|r| &r.diagnostics)
            .collect::<Vec<_>>()
    );
    // 绝对但缺失 → 保留行、Missing，不每轮报错（状态而非错误）
    let missing_env = home.join("env-logs");
    let ctx = ctx_for(
        &home,
        StartupEnv {
            codex_home: Some(missing_env.display().to_string()),
            claude_config_dir: None,
        },
        filesystem_probe(),
    );
    let out = resolve_sources(
        &ctx,
        &default_claude(),
        &default_codex(),
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    let env_row = out
        .roots
        .iter()
        .find(|r| {
            r.origins
                .iter()
                .any(|o| o.kind == OriginKind::Env("CODEX_HOME"))
        })
        .expect("缺失的环境根保留行");
    assert_eq!(env_row.state, ProbeState::Missing);
    assert!(
        !env_row.hidden_by_default_missing(),
        "环境根缺失不按默认根隐藏"
    );
}

/// 新格式 CODEX_HOME **追加**到默认根：默认双根与环境双根同时在场（§2 行为变更）。
#[test]
fn codex_home_appends_to_default_roots() {
    let home = temp_dir("codex-append");
    let env_base = home.join("portable-codex");
    std::fs::create_dir_all(env_base.join("sessions")).unwrap();
    let ctx = ctx_for(
        &home,
        StartupEnv {
            codex_home: Some(env_base.display().to_string()),
            claude_config_dir: None,
        },
        filesystem_probe(),
    );
    let out = resolve_sources(
        &ctx,
        &default_claude(),
        &default_codex(),
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    let codex: Vec<_> = out
        .roots
        .iter()
        .filter(|r| r.agent == AgentKind::Codex)
        .collect();
    assert_eq!(codex.len(), 4, "默认双根 + 环境双根: {:?}", codex);
    assert!(codex.iter().any(|r| r.display_path.contains(".codex")));
    assert!(
        codex
            .iter()
            .any(|r| r.display_path.contains("portable-codex"))
    );
    let env_row = codex
        .iter()
        .find(|r| r.origins[0].kind == OriginKind::Env("CODEX_HOME"))
        .unwrap();
    assert!(env_row.effective_enabled);
}

/// 大小写/尾分隔符/别名同根合并一行并保留来源标签（注入探测器模拟 canonical）。
#[test]
fn same_root_aliases_merge_with_labels() {
    let home = temp_dir("merge");
    let sessions = home.join(".codex/sessions");
    let probe = table_probe(vec![]);
    // 环境根与手动项用不同拼写指向同一物理根（假探测器会折算同一身份）
    let ctx = ctx_for(
        &home,
        StartupEnv {
            codex_home: Some(format!("{}/.codex", home.display())),
            claude_config_dir: None,
        },
        probe,
    );
    let mut codex = default_codex();
    codex.additional_paths = vec![tokenscope::settings::AdditionalSourcePath {
        path: sessions.display().to_string().to_uppercase(),
        enabled: true,
    }];
    let out = resolve_sources(
        &ctx,
        &default_claude(),
        &codex,
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    let merged = out
        .roots
        .iter()
        .find(|r| {
            r.agent == AgentKind::Codex && r.origins.iter().any(|o| o.kind == OriginKind::Manual)
        })
        .expect("同根必须合并（三标签并一行）");
    let kinds: Vec<&OriginKind> = merged.origins.iter().map(|o| &o.kind).collect();
    assert!(kinds.contains(&&OriginKind::Default));
    assert!(kinds.contains(&&OriginKind::Env("CODEX_HOME")));
    assert!(kinds.contains(&&OriginKind::Manual));
    // 稳定标签顺序：默认 < 环境 < 手动
    assert_eq!(merged.origins[0].kind, OriginKind::Default);
    assert_eq!(merged.origins[2].kind, OriginKind::Manual);
}

/// 未迁移旧格式不扩大读取：dir 取代默认根；Codex 环境根**替代**默认根。
#[test]
fn legacy_semantics_do_not_expand_reading() {
    let home = temp_dir("legacy");
    let probe = filesystem_probe();
    let ctx = ctx_for(&home, StartupEnv::default(), probe);
    // 模拟反序列化得到的旧格式：dir 在场且无任何新字段（legacy_format = true）
    let claude = SourceConfig {
        dir: Some("D:/legacy/claude".to_string()),
        legacy_format: true,
        ..Default::default()
    };
    let out = resolve_sources(
        &ctx,
        &claude,
        &default_codex(),
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    let claude_rows: Vec<_> = out
        .roots
        .iter()
        .filter(|r| r.agent == AgentKind::ClaudeCode)
        .collect();
    assert_eq!(claude_rows.len(), 1);
    assert_eq!(claude_rows[0].display_path, "D:/legacy/claude");
    assert!(
        claude_rows[0].mapping_path.is_none(),
        "A03：自定义根禁用映射"
    );
    assert!(
        claude_rows[0]
            .diagnostics
            .iter()
            .any(|d| d.contains("v1 语义")),
        "{:?}",
        claude_rows[0].diagnostics
    );

    let ctx = ctx_for(
        &home,
        StartupEnv {
            codex_home: Some(home.join("env-codex").display().to_string()),
            claude_config_dir: None,
        },
        filesystem_probe(),
    );
    // 旧格式的 codex（未迁移）：环境根替代默认根
    let codex_legacy = SourceConfig {
        legacy_format: true,
        ..Default::default()
    };
    let out = resolve_sources(
        &ctx,
        &default_claude(),
        &codex_legacy,
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    let codex_rows: Vec<_> = out
        .roots
        .iter()
        .filter(|r| r.agent == AgentKind::Codex)
        .collect();
    assert_eq!(codex_rows.len(), 2, "旧语义：环境根替代默认根");
    assert!(
        codex_rows
            .iter()
            .all(|r| r.display_path.contains("env-codex"))
    );
}

/// 映射绑定：默认根=默认绑定；环境根=未核实禁用；手动根=禁用；
/// 默认+手动同根合并时保留默认绑定并诊断（不混用）。
#[test]
fn mapping_bindings_follow_a03_and_env_degradation() {
    let home = temp_dir("mapping");
    let manual_dir = home.join("manual-claude");
    std::fs::create_dir_all(&manual_dir).unwrap();
    let ctx = ctx_for(&home, StartupEnv::default(), filesystem_probe());
    let mut claude = default_claude();
    claude.additional_paths = vec![tokenscope::settings::AdditionalSourcePath {
        path: manual_dir.display().to_string(),
        enabled: true,
    }];
    let out = resolve_sources(
        &ctx,
        &claude,
        &default_codex(),
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    let default_row = out
        .roots
        .iter()
        .find(|r| {
            r.agent == AgentKind::ClaudeCode
                && r.origins.len() == 1
                && r.origins[0].kind == OriginKind::Default
        })
        .unwrap();
    assert_eq!(
        default_row.mapping_path.as_ref().unwrap(),
        &home.join(".claude.json")
    );
    let manual_row = out
        .roots
        .iter()
        .find(|r| r.agent == AgentKind::ClaudeCode && r.origins[0].kind == OriginKind::Manual)
        .unwrap();
    assert!(manual_row.mapping_path.is_none());
    assert!(
        manual_row
            .diagnostics
            .iter()
            .any(|d| d.contains("手动追加"))
    );

    // 默认 + 手动合并同一物理根（探测同根）
    let probe = table_probe(vec![]);
    let ctx = ctx_for(&home, StartupEnv::default(), probe);
    let mut claude = default_claude();
    claude.additional_paths = vec![tokenscope::settings::AdditionalSourcePath {
        path: home
            .join(".claude/projects")
            .display()
            .to_string()
            .to_uppercase(),
        enabled: true,
    }];
    let out = resolve_sources(
        &ctx,
        &claude,
        &default_codex(),
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    let merged = out
        .roots
        .iter()
        .find(|r| r.agent == AgentKind::ClaudeCode && r.origins.len() == 2)
        .expect("同根合并");
    assert_eq!(
        merged.mapping_path.as_ref().unwrap(),
        &home.join(".claude.json"),
        "合并行优先已核实默认绑定"
    );
    assert!(merged.diagnostics.iter().any(|d| d.contains("默认绑定")));

    // CLAUDE_CONFIG_DIR 根：映射未核实 → 禁用 + 明确诊断
    let ctx = ctx_for(
        &home,
        StartupEnv {
            codex_home: None,
            claude_config_dir: Some(home.join("cfg-dir").display().to_string()),
        },
        filesystem_probe(),
    );
    let out = resolve_sources(
        &ctx,
        &default_claude(),
        &default_codex(),
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    let env_row = out
        .roots
        .iter()
        .find(|r| r.origins[0].kind == OriginKind::Env("CLAUDE_CONFIG_DIR"))
        .unwrap();
    assert!(env_row.mapping_path.is_none(), "未核实不借用默认映射");
    assert!(env_row.diagnostics.iter().any(|d| d.contains("未核实")));
}

/// 总开关与行开关组合：总开关只压有效值；auto_discover 只关自动 origin；
/// 排除项按规范化键匹配（大小写/尾分隔符别名仍命中）且不碰手动项。
#[test]
fn switches_and_exclusions_compose_without_rewriting_user_choices() {
    let home = temp_dir("switches");
    let manual_dir = home.join("manual");
    std::fs::create_dir_all(&manual_dir).unwrap();
    let ctx = ctx_for(&home, StartupEnv::default(), filesystem_probe());
    // 排除项用别名拼写：规范化后必须命中默认根
    let mut claude = default_claude();
    claude.excluded_auto_paths = vec![format!(
        "{}\\",
        home.join(".CLAUDE/projects")
            .display()
            .to_string()
            .replace('/', "\\")
    )];
    claude.additional_paths = vec![tokenscope::settings::AdditionalSourcePath {
        path: manual_dir.display().to_string(),
        enabled: true,
    }];
    let out = resolve_sources(
        &ctx,
        &claude,
        &default_codex(),
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    let default_row = out
        .roots
        .iter()
        .find(|r| r.agent == AgentKind::ClaudeCode && r.origins[0].kind == OriginKind::Default)
        .expect("被排除的默认根仍是候选行");
    assert!(!default_row.origins[0].enabled, "排除项只停用自动 origin");
    let manual_row = out
        .roots
        .iter()
        .find(|r| r.agent == AgentKind::ClaudeCode && r.origins[0].kind == OriginKind::Manual)
        .unwrap();
    assert!(manual_row.effective_enabled, "排除项不得波及手动行");

    // 总开关：压掉全部有效值，但 origin 用户选择不变
    let mut claude = default_claude();
    claude.enabled = false;
    let out = resolve_sources(
        &ctx,
        &claude,
        &default_codex(),
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    assert!(
        out.roots
            .iter()
            .filter(|r| r.agent == AgentKind::ClaudeCode)
            .all(|r| !r.effective_enabled),
        "总开关关闭后该应用全部行有效停用"
    );
    assert!(
        out.roots.iter().flat_map(|r| &r.origins).all(|o| o.enabled),
        "总开关不改写各行用户选择"
    );

    // auto_discover=false：自动 origin 停用，手动 origin 照常
    let mut claude = default_claude();
    claude.auto_discover = false;
    claude.additional_paths = vec![tokenscope::settings::AdditionalSourcePath {
        path: manual_dir.display().to_string(),
        enabled: true,
    }];
    let out = resolve_sources(
        &ctx,
        &claude,
        &default_codex(),
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    let manual_row = out
        .roots
        .iter()
        .find(|r| r.agent == AgentKind::ClaudeCode && r.origins[0].kind == OriginKind::Manual)
        .unwrap();
    assert!(manual_row.effective_enabled);
}

// ── 行操作：禁用 → 重解析 → 移除 → 开启 ─────────────────────────

/// §9.3 第 4 条的完整循环：默认+手动合并行的禁用→重启→移除→开启。
#[test]
fn row_disable_resolve_remove_enable_cycle_keeps_user_choices() {
    let home = temp_dir("row-cycle");
    let mut claude = default_claude();
    claude.additional_paths = vec![tokenscope::settings::AdditionalSourcePath {
        // 同一根（探测同身份）→ 与默认根合并成一行
        path: fake_canon(&home.join(".claude/projects"))
            .display()
            .to_string(),
        enabled: true,
    }];
    // 用表驱动探测器让默认根与手动项同身份（确定性，不依赖文件系统大小写语义）
    let probe = table_probe(vec![]);
    let ctx = ctx_for(&home, StartupEnv::default(), probe);
    let out = resolve_sources(
        &ctx,
        &claude,
        &default_codex(),
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    let merged = out
        .roots
        .iter()
        .find(|r| r.agent == AgentKind::ClaudeCode && r.origins.len() == 2)
        .expect("默认+手动合并行");
    let id = merged.id().to_string();
    let auto_path = merged
        .origins
        .iter()
        .find(|o| o.kind == OriginKind::Default)
        .unwrap()
        .config_path
        .clone();
    let manual_path = merged
        .origins
        .iter()
        .find(|o| o.kind == OriginKind::Manual)
        .unwrap()
        .config_path
        .clone();
    let row_paths = |manual: Vec<String>| tokenscope::source::discovery::RowConfigPaths {
        auto: vec![auto_path.clone()],
        manual,
    };

    // 禁用：自动 origin 进排除项 + 手动项 false
    set_row_enabled(&mut claude, &row_paths(vec![manual_path.clone()]), false).unwrap();
    assert!(
        claude
            .excluded_auto_paths
            .iter()
            .any(|p| tokenscope::settings::normalized_key(Path::new(p))
                == tokenscope::settings::normalized_key(Path::new(&auto_path))),
        "排除项必须写入: {:?}",
        claude.excluded_auto_paths
    );
    assert!(!claude.additional_paths[0].enabled);
    // 重启（重新解析）：行仍存在、有效停用、自动 origin 仍在配置里
    let out = resolve_sources(
        &ctx,
        &claude,
        &default_codex(),
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    let row = out
        .roots
        .iter()
        .find(|r| r.agent == AgentKind::ClaudeCode && r.id() == id)
        .expect("禁用不是删除：重启后行仍在");
    assert!(!row.effective_enabled);
    assert_eq!(row.origins.len(), 2, "停用不删除 origin");

    // 移除：只删手动项；自动排除项保持（行保持停用）
    remove_manual_from_row(&mut claude, &row_paths(vec![manual_path.clone()])).unwrap();
    assert!(claude.additional_paths.is_empty());
    assert!(
        !claude.excluded_auto_paths.is_empty(),
        "移除手动项后自动 origin 仍保持停用"
    );
    let out = resolve_sources(
        &ctx,
        &claude,
        &default_codex(),
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    let row = out
        .roots
        .iter()
        .find(|r| r.agent == AgentKind::ClaudeCode && r.id() == id)
        .unwrap();
    assert_eq!(row.origins.len(), 1);
    assert!(!row.effective_enabled);

    // 开启：排除项清空；被移除的手动项不得悄悄复活
    set_row_enabled(&mut claude, &row_paths(vec![]), true).unwrap();
    assert!(claude.excluded_auto_paths.is_empty());
    assert!(claude.additional_paths.is_empty(), "移除过的手动项不得复活");
    let out = resolve_sources(
        &ctx,
        &claude,
        &default_codex(),
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    let row = out
        .roots
        .iter()
        .find(|r| r.agent == AgentKind::ClaudeCode && r.id() == id)
        .unwrap();
    assert!(row.effective_enabled);
    assert_eq!(row.origins.len(), 1);
}

/// id 不匹配任何当前行 → "来源已变化，请重试"；纯自动行不可移除。
#[test]
fn row_operations_reject_unknown_ids_and_pure_auto_rows() {
    let home = temp_dir("row-reject");
    let _ = home;
    let mut claude = default_claude();
    // 空集合（无自动、无手动可操作项）→ 明确拒绝；纯自动行不可移除。
    let empty = tokenscope::source::discovery::RowConfigPaths::default();
    let err = set_row_enabled(&mut claude, &empty, false).unwrap_err();
    assert!(
        err.contains("没有可操作的配置项"),
        "空集合必须明确拒绝: {err}"
    );
    let err = remove_manual_from_row(&mut claude, &empty).unwrap_err();
    assert!(err.contains("没有可移除的手动目录"), "{err}");
}

/// 行 id 跨轮稳定；row_exists 按（agent, id）判定。
#[test]
fn row_ids_are_stable_across_re_resolution() {
    let home = temp_dir("row-id");
    let ctx = ctx_for(&home, StartupEnv::default(), filesystem_probe());
    let out1 = resolve_sources(
        &ctx,
        &default_claude(),
        &default_codex(),
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    let out2 = resolve_sources(
        &ctx,
        &default_claude(),
        &default_codex(),
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    for (a, b) in out1.roots.iter().zip(&out2.roots) {
        assert_eq!(a.id(), b.id());
    }
    let claude = out1
        .roots
        .iter()
        .find(|r| r.agent == AgentKind::ClaudeCode)
        .unwrap();
    assert!(row_exists(&out2.roots, AgentKind::ClaudeCode, claude.id()));
    assert!(!row_exists(&out2.roots, AgentKind::Codex, claude.id()));
}

// ── 每轮重新规范化、新目录出现与上下文签名 ───────────────────────

/// 新目录下一轮出现：同一上下文重解析即见（不缓存探测结果）。
#[test]
fn new_directory_appears_on_next_resolution() {
    let home = temp_dir("next-round");
    let probe = filesystem_probe();
    let ctx = ctx_for(&home, StartupEnv::default(), probe);
    let out1 = resolve_sources(
        &ctx,
        &default_claude(),
        &default_codex(),
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    let claude1 = out1
        .roots
        .iter()
        .find(|r| r.agent == AgentKind::ClaudeCode)
        .unwrap();
    assert_eq!(claude1.state, ProbeState::Missing, "前置：目录尚未创建");
    assert!(claude1.hidden_by_default_missing());
    std::fs::create_dir_all(home.join(".claude/projects")).unwrap();
    let out2 = resolve_sources(
        &ctx,
        &default_claude(),
        &default_codex(),
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    let claude2 = out2
        .roots
        .iter()
        .find(|r| r.agent == AgentKind::ClaudeCode)
        .unwrap();
    assert_eq!(
        claude2.state,
        ProbeState::Ready,
        "下一轮必须重新探测到新目录"
    );
    assert!(!claude2.hidden_by_default_missing());
}

/// 每轮重新规范化：探测结果变化立即反映；签名随有效变化而变、随重复解析不变。
#[test]
fn resolution_is_recomputed_and_signature_tracks_meaningful_changes() {
    let home = temp_dir("signature");
    // 可变探测器：模拟权限状态在两轮之间变化
    let deny = Arc::new(Mutex::new(false));
    let deny_clone = deny.clone();
    let sessions = home.join(".codex/sessions");
    let base = table_probe(vec![]);
    let probe: ProbeFn = Arc::new(move |path: &Path| {
        if *deny_clone.lock().unwrap() && path == sessions.as_path() {
            return RootProbe {
                state: ProbeState::NoPermission("denied".to_string()),
                canonical: None,
            };
        }
        (base)(path)
    });
    let ctx = ctx_for(&home, StartupEnv::default(), probe);
    let out1 = resolve_sources(
        &ctx,
        &default_claude(),
        &default_codex(),
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    let out2 = resolve_sources(
        &ctx,
        &default_claude(),
        &default_codex(),
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    assert_eq!(out1.signature, out2.signature, "相同输入签名必须相同");
    assert_eq!(out1.signature.len(), 16, "签名是 16 位十六进制");

    *deny.lock().unwrap() = true;
    let out3 = resolve_sources(
        &ctx,
        &default_claude(),
        &default_codex(),
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    assert_ne!(out1.signature, out3.signature, "探测状态变化必须进入签名");

    // 新增手动项：签名变化（根集合变化）
    *deny.lock().unwrap() = false;
    let mut claude = default_claude();
    claude.additional_paths = vec![tokenscope::settings::AdditionalSourcePath {
        path: home.join("extra").display().to_string(),
        enabled: true,
    }];
    let out4 = resolve_sources(
        &ctx,
        &claude,
        &default_codex(),
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    assert_ne!(out1.signature, out4.signature);
    assert!(
        out4.roots.iter().any(|r| r.display_path.ends_with("extra")),
        "手动追加项必须在结果里"
    );
}

/// 默认缺失行可隐藏，但环境/手动缺失行保留（§9.3 第 6 条）。
#[test]
fn only_default_missing_rows_are_hideable() {
    let home = temp_dir("hide");
    let env_base = home.join("env-codex");
    let probe = filesystem_probe();
    let ctx = ctx_for(
        &home,
        StartupEnv {
            codex_home: Some(env_base.display().to_string()),
            claude_config_dir: None,
        },
        probe,
    );
    let out = resolve_sources(
        &ctx,
        &default_claude(),
        &default_codex(),
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    let claude = out
        .roots
        .iter()
        .find(|r| r.agent == AgentKind::ClaudeCode)
        .unwrap();
    assert!(claude.hidden_by_default_missing(), "纯默认缺失行可隐藏");
    let env_row = out
        .roots
        .iter()
        .find(|r| r.origins[0].kind == OriginKind::Env("CODEX_HOME"))
        .unwrap();
    assert!(!env_row.hidden_by_default_missing(), "环境根缺失保留行");
}

// ── 跨应用冲突与同应用重叠（审阅 §23.2 补修）──────────────────────

/// 手动-only 配置：auto_discover 关闭，只保留给定追加根。
fn manual_only(paths: &[&std::path::Path]) -> SourceConfig {
    SourceConfig {
        enabled: true,
        dir: None,
        auto_discover: false,
        excluded_auto_paths: Vec::new(),
        additional_paths: paths
            .iter()
            .map(|p| tokenscope::settings::AdditionalSourcePath {
                path: p.display().to_string(),
                enabled: true,
            })
            .collect(),
        legacy_format: false,
    }
}

/// §4.1/§8.5：跨应用相同根双方标记冲突，诊断写明对方应用与路径。
#[test]
fn cross_app_same_root_marks_both_sides_conflicted() {
    let home = temp_dir("conflict-same");
    let shared = home.join("shared");
    std::fs::create_dir_all(&shared).unwrap();
    let ctx = ctx_for(&home, StartupEnv::default(), filesystem_probe());
    let out = resolve_sources(
        &ctx,
        &manual_only(&[&shared]),
        &manual_only(&[&shared]),
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    let shared_rows: Vec<_> = out
        .roots
        .iter()
        .filter(|r| norm(&r.display_path) == norm(&shared.to_string_lossy()))
        .collect();
    assert_eq!(shared_rows.len(), 2, "同根不同应用保持两行");
    assert_eq!(
        shared_rows.iter().map(|r| r.agent).collect::<Vec<_>>(),
        vec![AgentKind::ClaudeCode, AgentKind::Codex]
    );
    for row in &shared_rows {
        assert!(row.conflict_with.is_some(), "双方都标记冲突: {row:?}");
        assert!(
            row.diagnostics
                .iter()
                .any(|d| d.contains("冲突") && d.contains("本轮双方都不读取")),
            "诊断写明处置: {:?}",
            row.diagnostics
        );
    }
}

/// §4.1/§8.5：跨应用嵌套（Claude 父 / Codex 子）双方都标记，不只隔离一方。
#[test]
fn cross_app_nested_marks_both_sides_conflicted() {
    let home = temp_dir("conflict-nest");
    let parent = home.join("parent");
    let child = parent.join("sub");
    std::fs::create_dir_all(&child).unwrap();
    let ctx = ctx_for(&home, StartupEnv::default(), filesystem_probe());
    let out = resolve_sources(
        &ctx,
        &manual_only(&[&parent]),
        &manual_only(&[&child]),
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    for row in &out.roots {
        let is_overlapped = norm(&row.display_path) == norm(&parent.to_string_lossy())
            || norm(&row.display_path) == norm(&child.to_string_lossy());
        assert_eq!(
            row.conflict_with.is_some(),
            is_overlapped && row.effective_enabled,
            "只有有效启用的重叠根标记冲突: {row:?}"
        );
        if row.conflict_with.is_some() {
            assert!(
                row.diagnostics
                    .iter()
                    .any(|d| d.contains("本轮双方都不读取")),
                "冲突行有处置诊断: {:?}",
                row.diagnostics
            );
        }
    }
    let claude = out
        .roots
        .iter()
        .find(|r| r.agent == AgentKind::ClaudeCode)
        .unwrap();
    assert!(
        claude.diagnostics.iter().any(|d| d.contains("codex")),
        "诊断写明对方是 Codex: {:?}",
        claude.diagnostics
    );
}

/// 停用一侧不构成读取冲突：双方都不标记（停用根本就不读取）。
#[test]
fn disabled_side_does_not_conflict() {
    let home = temp_dir("conflict-disabled");
    let shared = home.join("shared");
    std::fs::create_dir_all(&shared).unwrap();
    let ctx = ctx_for(&home, StartupEnv::default(), filesystem_probe());
    let mut codex = manual_only(&[&shared]);
    codex.enabled = false;
    let out = resolve_sources(
        &ctx,
        &manual_only(&[&shared]),
        &codex,
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    for row in &out.roots {
        assert!(row.conflict_with.is_none(), "停用侧不参与冲突: {row:?}");
    }
}

/// §4.1：同应用嵌套属合法重叠——子根行加"与 X 重叠，文件由 X 读取"诊断，
/// 不标记冲突（数据由父根读取，不丢不重）。
#[test]
fn same_app_nested_child_gets_overlap_diagnostic() {
    let home = temp_dir("overlap-same");
    let parent = home.join("parent");
    let child = parent.join("sub");
    std::fs::create_dir_all(&child).unwrap();
    let ctx = ctx_for(&home, StartupEnv::default(), filesystem_probe());
    let out = resolve_sources(
        &ctx,
        &manual_only(&[&parent, &child]),
        &SourceConfig::default(),
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    let child_row = out
        .roots
        .iter()
        .find(|r| norm(&r.display_path) == norm(&child.to_string_lossy()))
        .unwrap();
    assert!(child_row.conflict_with.is_none(), "同应用不标冲突");
    assert!(
        child_row
            .diagnostics
            .iter()
            .any(|d| d.contains("重叠") && d.contains("文件由")),
        "子根行有重叠诊断: {:?}",
        child_row.diagnostics
    );
    let parent_row = out
        .roots
        .iter()
        .find(|r| norm(&r.display_path) == norm(&parent.to_string_lossy()))
        .unwrap();
    assert!(
        !parent_row
            .diagnostics
            .iter()
            .any(|d| d.contains("重叠，文件由")),
        "父根是读取方，无重叠诊断: {:?}",
        parent_row.diagnostics
    );
}

// ── WB04：两个 WorkBuddy 的默认根、开关与跨应用冲突 ──────────────

fn ctx(home: &Path) -> DiscoveryContext {
    ctx_for(home, StartupEnv::default(), filesystem_probe())
}

#[test]
fn workbuddy_default_roots_are_fixed_candidates() {
    let home = temp_dir("wb-defaults");
    std::fs::create_dir_all(home.join(".workbuddy/projects")).unwrap();
    std::fs::create_dir_all(home.join(".workbuddy-ai/projects")).unwrap();
    let out = resolve_sources(
        &ctx(&home),
        &SourceConfig::default(),
        &SourceConfig::default(),
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    let kinds: Vec<AgentKind> = out.roots.iter().map(|r| r.agent).collect();
    for kind in [
        AgentKind::ClaudeCode,
        AgentKind::Codex,
        AgentKind::WorkBuddy,
        AgentKind::WorkBuddyAi,
    ] {
        assert!(
            kinds.contains(&kind),
            "四应用的默认根都必须出现在候选集：{kinds:?}"
        );
    }
    let wb = out
        .roots
        .iter()
        .find(|r| r.agent == AgentKind::WorkBuddy)
        .unwrap();
    assert_eq!(
        PathBuf::from(&wb.display_path),
        home.join(".workbuddy/projects"),
        "默认根 = ~/.workbuddy/projects"
    );
    assert!(
        wb.origins
            .iter()
            .all(|o| matches!(o.kind, OriginKind::Default)),
        "WorkBuddy 没有环境变量派生根"
    );
    let wba = out
        .roots
        .iter()
        .find(|r| r.agent == AgentKind::WorkBuddyAi)
        .unwrap();
    assert_eq!(
        PathBuf::from(&wba.display_path),
        home.join(".workbuddy-ai/projects")
    );
    std::fs::remove_dir_all(&home).ok();
}

#[test]
fn workbuddy_total_switch_and_auto_discover_gates() {
    let home = temp_dir("wb-switch");
    std::fs::create_dir_all(home.join(".workbuddy/projects")).unwrap();
    // 总开关关闭：行保留但 effective_enabled=false
    let wb_off = SourceConfig {
        enabled: false,
        ..SourceConfig::default()
    };
    let out = resolve_sources(
        &ctx(&home),
        &SourceConfig::default(),
        &SourceConfig::default(),
        &wb_off,
        &SourceConfig::default(),
    );
    let wb = out
        .roots
        .iter()
        .find(|r| r.agent == AgentKind::WorkBuddy)
        .unwrap();
    assert!(!wb.effective_enabled, "总开关关闭 → 行不进采集");
    // 自动发现关闭：默认根被关掉（仍显示行）
    let wb_no_auto = SourceConfig {
        auto_discover: false,
        ..SourceConfig::default()
    };
    let out = resolve_sources(
        &ctx(&home),
        &SourceConfig::default(),
        &SourceConfig::default(),
        &wb_no_auto,
        &SourceConfig::default(),
    );
    let wb = out
        .roots
        .iter()
        .find(|r| r.agent == AgentKind::WorkBuddy)
        .unwrap();
    assert!(!wb.effective_enabled, "自动发现关闭 → 默认根不采集");
    std::fs::remove_dir_all(&home).ok();
}

#[test]
fn workbuddy_root_shared_with_claude_is_conflicted() {
    // 跨应用冲突：把 ~/.workbuddy 追加给 Claude → 与 WorkBuddy 默认根的父目录嵌套。
    let home = temp_dir("wb-conflict");
    std::fs::create_dir_all(home.join(".workbuddy/projects")).unwrap();
    let claude_shared = SourceConfig {
        additional_paths: vec![tokenscope::settings::AdditionalSourcePath {
            path: home.join(".workbuddy").display().to_string(),
            enabled: true,
        }],
        ..SourceConfig::default()
    };
    let out = resolve_sources(
        &ctx(&home),
        &claude_shared,
        &SourceConfig::default(),
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    let claude_row = out
        .roots
        .iter()
        .find(|r| r.agent == AgentKind::ClaudeCode)
        .unwrap();
    let wb_row = out
        .roots
        .iter()
        .find(|r| r.agent == AgentKind::WorkBuddy)
        .unwrap();
    assert!(
        claude_row.conflict_with.is_some(),
        "Claude 手动根与 WorkBuddy 默认根嵌套 → 冲突"
    );
    assert!(
        wb_row.conflict_with.is_some(),
        "WorkBuddy 默认根同样被标记（无先扫描者得）"
    );
    assert!(!claude_row.effective_enabled || claude_row.conflict_with.is_some());
    std::fs::remove_dir_all(&home).ok();
}

#[test]
fn workbuddy_missing_default_root_is_hidden_or_missing_row() {
    let home = temp_dir("wb-missing");
    // 不创建目录：缺失默认根（纯自动 + Missing）→ hidden_by_default_missing
    let out = resolve_sources(
        &ctx(&home),
        &SourceConfig::default(),
        &SourceConfig::default(),
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    let wb = out
        .roots
        .iter()
        .find(|r| r.agent == AgentKind::WorkBuddy)
        .unwrap();
    assert_eq!(wb.state, ProbeState::Missing);
    assert!(wb.hidden_by_default_missing(), "纯默认缺失根可隐藏");
    std::fs::remove_dir_all(&home).ok();
}
