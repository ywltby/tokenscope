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

/// Windows 上创建目录符号链接需要 SeCreateSymbolicLinkPrivilege（管理员或
/// 开发者模式）。**只有**确证的原始错误码 1314（`ERROR_PRIVILEGE_NOT_HELD`）
/// 才归为"环境无符号链接特权"；普通访问拒绝、磁盘、防病毒、竞态等其他错误
/// 如实失败，不冒充"缺权限"。
///
/// 豁免边界：
/// - **本地**（`CI` 未设置）：无特权时允许跳过，但必须打印可识别的
///   `SYMLINK-TESTS-SKIPPED:` 标记——未执行项不冒充"全部真实断言通过"；
/// - **CI**（`CI` 设置，GitHub runner 必有特权）：无特权**直接 panic**——
///   环境不满足就是失败，不允许静默放行，测试架不依赖"return 记为 pass"。
///
/// 探针只创建/清理自己本次新建且确认归属的路径；创建失败后的清理错误与
/// 原始创建错误一并报告（原始错误为主，不被清理错误覆盖）。
#[cfg(windows)]
fn windows_symlink_privilege_available() -> bool {
    let ci = std::env::var_os("CI");
    match windows_symlink_probe_result() {
        Ok(()) => true,
        Err(e) => symlink_exemption_decision(&e, ci.as_deref()),
    }
}

/// 判定：探针错误是否进入本地能力豁免。纯函数，最小测试直接驱动。
/// 只有原始码 1314 才豁免；其他错误（含 `ErrorKind::PermissionDenied` 的
/// 普通访问拒绝）必须如实失败。`ci_environment` 由调用方传入进程的 `CI`
/// 环境变量（测试注入，不改进程环境）。
#[cfg(windows)]
fn symlink_exemption_decision(
    e: &std::io::Error,
    ci_environment: Option<&std::ffi::OsStr>,
) -> bool {
    if e.raw_os_error() == Some(1314) {
        if ci_environment.is_some() {
            panic!(
                "CI 环境缺少符号链接创建特权（os error 1314）：\
                 runner 必须能真实构造 junction 并执行隔离断言"
            );
        }
        eprintln!(
            "SYMLINK-TESTS-SKIPPED: 本机无 SeCreateSymbolicLinkPrivilege（os error 1314），\
             symlink 前置用例未执行；GitHub runner 有特权会照常执行真实断言"
        );
        return false;
    }
    panic!("符号链接能力探针失败（非符号链接特权问题，必须如实失败）：{e}");
}

/// 探针：创建/清理本次自建且确认归属的路径。创建失败时保留**原始创建错误**
/// 为主、清理错误附加打印，不被覆盖；创建成功但清理失败同样如实失败。
#[cfg(windows)]
fn windows_symlink_probe_result() -> std::io::Result<()> {
    let workspace = temp_dir("symlink-probe");
    let probe_target = workspace.join("target");
    std::fs::create_dir_all(&probe_target)
        .unwrap_or_else(|e| panic!("符号链接探针：无法创建目标目录 {:?}: {e}", probe_target));
    let probe_link = workspace.join("link");
    match std::os::windows::fs::symlink_dir(&probe_target, &probe_link) {
        Ok(()) => {
            // 清理失败是真实问题：留下符号链接会污染后续轮次，如实失败。
            if let Err(e) = std::fs::remove_dir_all(&workspace) {
                return Err(std::io::Error::other(format!(
                    "符号链接探针清理失败（{:?}）：{e}",
                    workspace
                )));
            }
            Ok(())
        }
        Err(created) => {
            if let Err(cleanup) = std::fs::remove_dir_all(&workspace) {
                eprintln!(
                    "符号链接探针清理失败（{:?}）：{cleanup}（原始创建错误：{created}）",
                    workspace
                );
            }
            Err(created)
        }
    }
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
            acceptance_isolated: false,
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
            acceptance_isolated: false,
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
            acceptance_isolated: false,
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
            acceptance_isolated: false,
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

/// RC10/AC10：验收隔离上下文里环境派生根绝不成为候选——`CLAUDE_CONFIG_DIR`
/// 与 `CODEX_HOME` 在启动环境里指向真实用户目录时，透传它们会让验收实例读到
/// 真实来源日志（隔离根之外的实际读写路径）。修复前：环境根出现在候选集；
/// 修复后：只保留隔离根默认根，且诊断明确记录"已忽略"。
#[test]
fn acceptance_isolated_context_drops_env_derived_roots() {
    let home = temp_dir("acceptance-env");
    let env_codex = home.join("real-codex-home");
    let env_claude = home.join("real-claude-config");
    std::fs::create_dir_all(env_codex.join("sessions")).unwrap();
    std::fs::create_dir_all(env_claude.join("projects")).unwrap();
    let ctx = DiscoveryContext {
        env: StartupEnv {
            codex_home: Some(env_codex.display().to_string()),
            claude_config_dir: Some(env_claude.display().to_string()),
            acceptance_isolated: true,
        },
        ..ctx_for(&home, StartupEnv::default(), filesystem_probe())
    };
    let out = resolve_sources(
        &ctx,
        &default_claude(),
        &default_codex(),
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    for kind in [
        OriginKind::Env("CODEX_HOME"),
        OriginKind::Env("CLAUDE_CONFIG_DIR"),
    ] {
        assert!(
            !out.roots
                .iter()
                .any(|r| r.origins.iter().any(|o| o.kind == kind)),
            "验收隔离上下文不得出现环境候选根 {kind:?}：{:?}",
            out.roots
        );
    }
    let codex = out
        .roots
        .iter()
        .filter(|r| r.agent == AgentKind::Codex)
        .count();
    let claude = out
        .roots
        .iter()
        .filter(|r| r.agent == AgentKind::ClaudeCode)
        .count();
    assert_eq!(codex, 2, "只保留隔离根默认双根：{:?}", out.roots);
    assert_eq!(claude, 1, "只保留隔离根默认单根：{:?}", out.roots);
    let diagnostics: Vec<_> = out
        .roots
        .iter()
        .flat_map(|r| r.diagnostics.iter())
        .collect();
    assert!(
        diagnostics
            .iter()
            .any(|d| d.contains("CODEX_HOME") && d.contains("验收隔离")),
        "缺少 CODEX_HOME 忽略诊断：{diagnostics:?}"
    );
    assert!(
        diagnostics
            .iter()
            .any(|d| d.contains("CLAUDE_CONFIG_DIR") && d.contains("验收隔离")),
        "缺少 CLAUDE_CONFIG_DIR 忽略诊断：{diagnostics:?}"
    );
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
            acceptance_isolated: false,
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
            acceptance_isolated: false,
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

/// RC10/AC10 + 旧格式分支：未迁移 Codex 配置（空 `dir`）在验收隔离上下文里
/// 也不能把 `CODEX_HOME` 指向的真实目录当默认根读取——旧语义"环境根替代默认
/// 根"只在普通模式生效。注入探测器与假环境验证，不读真实环境变量。
/// 修复前：旧格式分支不看 acceptance_isolated，环境根直接成为候选。
#[test]
fn acceptance_isolated_codex_legacy_branch_ignores_codex_home() {
    let home = temp_dir("acceptance-legacy");
    let env_codex = home.join("real-portable-codex");
    std::fs::create_dir_all(env_codex.join("sessions")).unwrap();
    let ctx = DiscoveryContext {
        env: StartupEnv {
            codex_home: Some(env_codex.display().to_string()),
            claude_config_dir: None,
            acceptance_isolated: true,
        },
        ..ctx_for(&home, StartupEnv::default(), filesystem_probe())
    };
    // 旧格式 Codex：dir 缺失（等价于迁移失败窗口期的空 dir 形态）
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
    let codex: Vec<_> = out
        .roots
        .iter()
        .filter(|r| r.agent == AgentKind::Codex)
        .collect();
    assert!(
        codex
            .iter()
            .all(|r| !r.display_path.contains("real-portable-codex")),
        "验收隔离上下文不得读取 CODEX_HOME 指向的真实目录：{:?}",
        codex
    );
    assert_eq!(
        codex.len(),
        2,
        "回退到隔离根默认双根（隔离根下的 sessions/archived_sessions）：{:?}",
        codex
    );
    assert!(
        codex
            .iter()
            .all(|r| r.origins[0].kind == OriginKind::Default),
        "旧格式分支保留 Default 出处分类与 v1 诊断：{:?}",
        codex
    );
    assert!(
        codex
            .iter()
            .all(|r| r.diagnostics.iter().any(|d| d.contains("v1 语义"))),
        "{:?}",
        codex
    );

    // 普通模式对照：同形态配置 + 同环境，旧语义原样保留（环境根替代默认根）。
    let plain_ctx = ctx_for(
        &home,
        StartupEnv {
            codex_home: Some(env_codex.display().to_string()),
            claude_config_dir: None,
            acceptance_isolated: false,
        },
        filesystem_probe(),
    );
    let plain = resolve_sources(
        &plain_ctx,
        &default_claude(),
        &codex_legacy,
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    let plain_codex: Vec<_> = plain
        .roots
        .iter()
        .filter(|r| r.agent == AgentKind::Codex)
        .collect();
    assert_eq!(plain_codex.len(), 2, "普通模式旧语义不变");
    assert!(
        plain_codex
            .iter()
            .all(|r| r.display_path.contains("real-portable-codex")),
        "普通模式环境根替代默认根：{:?}",
        plain_codex
    );
}

/// RC10/AC10：验收隔离上下文里，旧格式显式目录与手动追加目录只允许落在
/// 验收根内的真实路径——指向隔离根之外的目录（真实用户数据、任意磁盘路径）
/// 一律不进入候选集并留诊断；普通产品模式同形态配置的来源语义原样保留。
/// 注入探测器与假路径验证，不读真实磁盘。
#[test]
fn acceptance_isolated_rejects_outside_legacy_and_manual_roots() {
    let home = temp_dir("acceptance-paths");
    // 隔离根用真实临时目录（五轮复核：约束按 canonicalize 判定，根必须可解析；
    // 不存在的根会 fail closed——单独的反例覆盖该分支）
    let acc_root = temp_dir("acceptance-paths-root");
    std::fs::create_dir_all(acc_root.join("sources/claude")).unwrap();
    // 隔离根之外的目录也用真实临时目录（存在于磁盘，但位于根外）
    let outside_base = temp_dir("acceptance-paths-outside");
    let outside_dir = outside_base.join(".claude/projects").display().to_string();
    let outside_manual = outside_base.join(".codex/sessions").display().to_string();
    std::fs::create_dir_all(&outside_dir).unwrap();
    std::fs::create_dir_all(&outside_manual).unwrap();
    let probe = filesystem_probe();
    // 验收上下文：显式 dir（旧格式）与手动追加都指向隔离根之外
    let legacy_claude = SourceConfig {
        dir: Some(outside_dir.clone()),
        legacy_format: true,
        ..Default::default()
    };
    let legacy_codex = SourceConfig {
        dir: Some(outside_manual.clone()),
        legacy_format: true,
        ..Default::default()
    };
    let mut manual_claude = default_claude();
    manual_claude.additional_paths = vec![tokenscope::settings::AdditionalSourcePath {
        path: outside_manual.clone(),
        enabled: true,
    }];
    let ctx = DiscoveryContext {
        env: StartupEnv {
            codex_home: None,
            claude_config_dir: None,
            acceptance_isolated: true,
        },
        acceptance_root: Some(acc_root.clone()),
        ..ctx_for(&home, StartupEnv::default(), probe.clone())
    };
    let out = resolve_sources(
        &ctx,
        &legacy_claude,
        &legacy_codex,
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    for agent_label in ["outside legacy dir", "outside manual path"] {
        let needle = if agent_label.contains("legacy") {
            outside_dir.clone()
        } else {
            outside_manual.clone()
        };
        assert!(
            !out.roots.iter().any(|r| r.display_path.contains(&needle)),
            "验收隔离上下文不得出现隔离根之外的 {agent_label}：{:?}",
            out.roots
        );
    }
    let claude_rows: Vec<_> = out
        .roots
        .iter()
        .filter(|r| r.agent == AgentKind::ClaudeCode)
        .collect();
    assert!(
        claude_rows.iter().any(|r| r
            .diagnostics
            .iter()
            .any(|d| d.contains("验收隔离") && d.contains("来源"))),
        "旧格式显式目录被丢弃时必须留验收隔离诊断：{:?}",
        claude_rows
    );

    // 手动追加（新格式 claude/codex/workbuddy 同样约束）：隔离根外 → 丢弃
    let mut manual_codex = default_codex();
    manual_codex.additional_paths = vec![tokenscope::settings::AdditionalSourcePath {
        path: outside_dir.to_string(),
        enabled: true,
    }];
    let out_manual = resolve_sources(
        &ctx,
        &manual_claude,
        &manual_codex,
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    assert!(
        !out_manual
            .roots
            .iter()
            .any(|r| r.display_path.contains("acceptance-paths-outside")),
        "隔离根之外的手动追加目录不得进入候选集：{:?}",
        out_manual.roots
    );

    // 隔离根内的路径：允许（真实路径落在验收根内）
    let inside_claude = SourceConfig {
        dir: Some(acc_root.join("sources/claude").display().to_string()),
        legacy_format: true,
        ..Default::default()
    };
    let inside_out = resolve_sources(
        &ctx,
        &inside_claude,
        &legacy_codex,
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    assert!(
        inside_out
            .roots
            .iter()
            .any(|r| r.display_path == acc_root.join("sources/claude").display().to_string()),
        "验收根内的显式目录允许：{:?}",
        inside_out.roots
    );

    // 普通产品模式对照：同形态配置、无隔离标志 → 旧格式显式目录与手动追加
    // 的来源语义原样保留（dir 取代默认根、手动追加在场）。
    let plain_ctx = ctx_for(&home, StartupEnv::default(), probe);
    let plain = resolve_sources(
        &plain_ctx,
        &legacy_claude,
        &legacy_codex,
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    assert!(
        plain.roots.iter().any(|r| r.display_path == outside_dir),
        "普通模式旧格式显式目录语义不变：{:?}",
        plain.roots
    );
    let plain_manual = resolve_sources(
        &plain_ctx,
        &manual_claude,
        &manual_codex,
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    assert!(
        plain_manual
            .roots
            .iter()
            .any(|r| r.display_path.contains("acceptance-paths-outside")),
        "普通模式手动追加目录语义不变：{:?}",
        plain_manual.roots
    );
}

/// RC10/AC10 五轮复核：隔离判断必须按**真实路径**（canonicalize）判定——
/// `..` 组件逃逸（字符串在根内、真实位置在根外）、junction/symlink 指向根外、
/// 断链/无法解析（无法证明）都不得进入候选集。判断先行：被丢弃的路径不做
/// 来源探测。真实临时目录 + 真实 junction + 真实解析器验证；外部哨兵由本
/// 测试自建自拆，不触碰真实用户数据。
#[test]
fn acceptance_isolation_resolves_real_paths_and_rejects_escapes() {
    #[cfg(windows)]
    if !windows_symlink_privilege_available() {
        eprintln!(
            "跳过：Windows 无符号链接创建特权（SeCreateSymbolicLinkPrivilege），\
             无法构造 junction/断链前置；GitHub runner 有特权照常执行"
        );
        return;
    }
    let home = temp_dir("acceptance-real-home");
    let root = temp_dir("acceptance-real-root");
    std::fs::create_dir_all(root.join("sources/claude/projects")).unwrap();
    // 外部哨兵：本次测试自建的独立目录（非真实用户数据），验证后自拆
    let outside = temp_dir("acceptance-real-outside");
    std::fs::create_dir_all(outside.join("sessions")).unwrap();
    let sentinel = outside.join("sessions/sentinel.jsonl");
    std::fs::write(&sentinel, "SENTINEL\n").unwrap();

    // 1) `..` 组件逃逸：字符串在根内，真实位置在根外
    //    projects 上四级 = TEMP（root 的父级），再进外部哨兵目录
    let dotdot_dir = root
        .join("sources")
        .join("claude")
        .join("projects")
        .join("..")
        .join("..")
        .join("..")
        .join("..")
        .join(outside.file_name().unwrap())
        .join("sessions");
    assert!(
        dotdot_dir.starts_with(&root),
        "前置：组件 starts_with 把 .. 形态误判在根内"
    );
    assert!(
        !std::fs::canonicalize(&dotdot_dir)
            .unwrap()
            .starts_with(std::fs::canonicalize(&root).unwrap()),
        "前置：.. 形态的真实位置在根外"
    );

    // 2) junction 指向根外：字符串在根内，解析后在外部
    let junction = root.join("sources/escape-link");
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&outside, &junction).unwrap();
    #[cfg(not(windows))]
    std::os::unix::fs::symlink(&outside, &junction).unwrap();

    // 3) 断链：无法证明 → 拒绝
    let dangling = root.join("sources/dangling-link");
    let dangling_target = root.join("sources/gone");
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&dangling_target, &dangling).unwrap();
    #[cfg(not(windows))]
    std::os::unix::fs::symlink(&dangling_target, &dangling).unwrap();
    assert!(
        std::fs::canonicalize(&dangling).is_err(),
        "前置：断链无法解析"
    );

    let ctx = DiscoveryContext {
        env: StartupEnv {
            codex_home: None,
            claude_config_dir: None,
            acceptance_isolated: true,
        },
        acceptance_root: Some(root.clone()),
        ..ctx_for(&home, StartupEnv::default(), filesystem_probe())
    };

    // 旧格式显式目录：.. 逃逸形态
    let legacy_claude = SourceConfig {
        dir: Some(dotdot_dir.display().to_string()),
        legacy_format: true,
        ..Default::default()
    };
    // 手动追加：junction 与断链两种形态（claude + codex）
    let mut manual_claude = default_claude();
    manual_claude.additional_paths = vec![
        tokenscope::settings::AdditionalSourcePath {
            path: junction.display().to_string(),
            enabled: true,
        },
        tokenscope::settings::AdditionalSourcePath {
            path: dangling.display().to_string(),
            enabled: true,
        },
    ];
    let out = resolve_sources(
        &ctx,
        &legacy_claude,
        &SourceConfig::default(),
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    assert!(
        !out.roots
            .iter()
            .any(|r| r.display_path.contains("acceptance-real-outside")
                || r.display_path.contains("..")),
        ".. 逃逸形态不得进入候选集：{:?}",
        out.roots
    );
    let out_manual = resolve_sources(
        &ctx,
        &manual_claude,
        &SourceConfig::default(),
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    assert!(
        !out_manual.roots.iter().any(|r| {
            r.display_path.contains("escape-link")
                || r.display_path.contains("dangling-link")
                || r.display_path.contains("acceptance-real-outside")
        }),
        "junction/断链手动目录不得进入候选集：{:?}",
        out_manual.roots
    );
    // 哨兵未被读取侧改动（约束只影响候选集，不写任何数据）
    assert_eq!(std::fs::read_to_string(&sentinel).unwrap(), "SENTINEL\n");

    // 正例：隔离根内的真实目录允许
    let inside_claude = SourceConfig {
        dir: Some(root.join("sources/claude").display().to_string()),
        legacy_format: true,
        ..Default::default()
    };
    let inside_out = resolve_sources(
        &ctx,
        &inside_claude,
        &SourceConfig::default(),
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    assert!(
        inside_out
            .roots
            .iter()
            .any(|r| std::fs::canonicalize(&r.display_path)
                .map(|c| c.starts_with(std::fs::canonicalize(&root).unwrap()))
                .unwrap_or(false)),
        "隔离根内的显式目录允许：{:?}",
        inside_out.roots
    );

    // 普通产品模式对照：同形态配置原样保留（.. 形态与 junction 都按用户输入
    // 进候选集，语义不变——约束只在验收隔离上下文生效）。
    let plain_ctx = ctx_for(&home, StartupEnv::default(), filesystem_probe());
    let plain = resolve_sources(
        &plain_ctx,
        &legacy_claude,
        &SourceConfig::default(),
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    assert!(
        plain
            .roots
            .iter()
            .any(|r| r.display_path == dotdot_dir.display().to_string()),
        "普通模式 .. 形态显式目录语义不变：{:?}",
        plain.roots
    );
    let plain_manual = resolve_sources(
        &plain_ctx,
        &manual_claude,
        &SourceConfig::default(),
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    assert!(
        plain_manual
            .roots
            .iter()
            .any(|r| r.display_path == junction.display().to_string()),
        "普通模式 junction 手动目录语义不变：{:?}",
        plain_manual.roots
    );

    // 现场自拆：外部哨兵目录与隔离根由本测试创建并清理
    let _ = std::fs::remove_dir_all(&outside);
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&home);
}

/// RC10/AC10 七轮复核缺口 2：默认来源与显式来源被拒后的**回退路径**也必须
/// 在探测前满足隔离约束——回退的默认根指向根外时，该行停用（Missing、
/// enabled=false）且绝不触发探测器；隔离标志置位但根缺失/无法证明时，全部
/// 候选停用（不回退为无限制模式）。注入计数探测器验证"不探测"。
#[test]
fn acceptance_isolation_gates_fallback_roots_before_probing() {
    let home = temp_dir("acceptance-fallback-home");
    let root = temp_dir("acceptance-fallback-root");
    std::fs::create_dir_all(root.join("sources/claude")).unwrap();
    // 根外的默认根（模拟 production 之外的可控注入：默认锚点不在验收根内）
    let outside_default = temp_dir("acceptance-fallback-outside");
    std::fs::create_dir_all(outside_default.join("projects")).unwrap();

    // 可计数探测器：记录每个被探测的路径
    let probed: Arc<Mutex<Vec<PathBuf>>> = Arc::new(Mutex::new(Vec::new()));
    let probed_for_probe = probed.clone();
    let counting_probe: ProbeFn = Arc::new(move |path: &Path| {
        probed_for_probe.lock().unwrap().push(path.to_path_buf());
        filesystem_probe()(path)
    });

    let ctx = DiscoveryContext {
        claude_default_root: outside_default.join("projects"),
        env: StartupEnv {
            codex_home: None,
            claude_config_dir: None,
            acceptance_isolated: true,
        },
        acceptance_root: Some(root.clone()),
        ..ctx_for(&home, StartupEnv::default(), counting_probe.clone())
    };
    let out = resolve_sources(
        &ctx,
        &default_claude(),
        &default_codex(),
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    let claude_row = out
        .roots
        .iter()
        .find(|r| r.agent == AgentKind::ClaudeCode)
        .expect("Claude 默认行保留（停用态）");
    assert!(
        !claude_row.effective_enabled,
        "根外默认根必须停用：{:?}",
        claude_row
    );
    assert_eq!(claude_row.state, ProbeState::Missing, "停用行不探测");
    assert!(
        claude_row
            .diagnostics
            .iter()
            .any(|d| d.contains("验收隔离") && d.contains("停用")),
        "停用行必须带验收隔离诊断：{:?}",
        claude_row
    );
    // 根外的默认根绝不被探测
    assert!(
        !probed
            .lock()
            .unwrap()
            .iter()
            .any(|p| p.starts_with(&outside_default)),
        "隔离根之外的默认根不得被探测：{:?}",
        probed.lock().unwrap()
    );

    // 隔离标志置位但根缺失 → fail closed：全部候选停用，不回退为无限制。
    let missing_root_ctx = DiscoveryContext {
        claude_default_root: root.join("sources/claude"),
        env: StartupEnv {
            codex_home: None,
            claude_config_dir: None,
            acceptance_isolated: true,
        },
        acceptance_root: Some(temp_dir("acceptance-fallback-missing-root")),
        ..ctx_for(&home, StartupEnv::default(), counting_probe.clone())
    };
    let out_missing = resolve_sources(
        &missing_root_ctx,
        &default_claude(),
        &default_codex(),
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    for row in &out_missing.roots {
        assert!(
            !row.effective_enabled,
            "根缺失时所有候选停用（不回退无限制）：{:?}",
            row
        );
    }
    assert!(
        !out_missing
            .roots
            .iter()
            .any(|r| r.agent == AgentKind::ClaudeCode && r.state == ProbeState::Ready),
        "根缺失时不得有 Ready 行：{:?}",
        out_missing.roots
    );

    // 正例对照：默认根在隔离根内 → 正常探测与启用
    let inside_ctx = DiscoveryContext {
        claude_default_root: root.join("sources/claude"),
        env: StartupEnv {
            codex_home: None,
            claude_config_dir: None,
            acceptance_isolated: true,
        },
        acceptance_root: Some(root.clone()),
        ..ctx_for(&home, StartupEnv::default(), counting_probe)
    };
    let out_inside = resolve_sources(
        &inside_ctx,
        &default_claude(),
        &SourceConfig::default(),
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    let inside_row = out_inside
        .roots
        .iter()
        .find(|r| r.agent == AgentKind::ClaudeCode)
        .expect("隔离根内默认行在场");
    assert!(
        inside_row.effective_enabled,
        "隔离根内默认根正常启用：{:?}",
        inside_row
    );

    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&outside_default);
}

/// RC10/AC10 八轮复核问题 2：`acceptance_isolated=true` 且 `acceptance_root=
/// None` 必须 fail closed——隔离标志声明了验收上下文却没有根，无法证明任何
/// 路径在根内，绝不能按普通产品模式无限制放行。分别覆盖 None、真正不存在
/// 的根、无法解析的根（假路径形态），并用计数探测器证明被拒路径未被探测；
/// 隔离根内正例正常；普通产品模式（isolated=false）语义原样保留。
#[test]
fn acceptance_isolated_without_root_fails_closed() {
    #[cfg(windows)]
    if !windows_symlink_privilege_available() {
        eprintln!(
            "跳过：Windows 无符号链接创建特权（SeCreateSymbolicLinkPrivilege），\
             无法构造断链 junction 前置；GitHub runner 有特权照常执行"
        );
        return;
    }
    let home = temp_dir("acc8-home");
    let outside_default = temp_dir("acc8-outside");
    std::fs::create_dir_all(outside_default.join("projects")).unwrap();

    let probed: Arc<Mutex<Vec<PathBuf>>> = Arc::new(Mutex::new(Vec::new()));
    let counting_probe: ProbeFn = {
        let probed = probed.clone();
        Arc::new(move |path: &Path| {
            probed.lock().unwrap().push(path.to_path_buf());
            filesystem_probe()(path)
        })
    };

    // 1) acceptance_root = None + isolated=true：fail closed
    let ctx_none = DiscoveryContext {
        claude_default_root: outside_default.join("projects"),
        env: StartupEnv {
            codex_home: None,
            claude_config_dir: None,
            acceptance_isolated: true,
        },
        acceptance_root: None,
        ..ctx_for(&home, StartupEnv::default(), counting_probe.clone())
    };
    let out_none = resolve_sources(
        &ctx_none,
        &default_claude(),
        &default_codex(),
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    for row in &out_none.roots {
        assert!(
            !row.effective_enabled,
            "隔离标志置位但根缺失（None）：全部候选停用，不回退无限制：{:?}",
            row
        );
        assert_eq!(row.state, ProbeState::Missing, "停用行不探测：{:?}", row);
    }
    assert!(
        !probed
            .lock()
            .unwrap()
            .iter()
            .any(|p| p.starts_with(&outside_default)),
        "根缺失时根外默认根不得被探测：{:?}",
        probed.lock().unwrap()
    );

    // 2) 真正不存在的根：fail closed。在本次创建的临时父目录下使用尚未
    //    创建的子路径（temp_dir 会创建目录，因此不能用它表示"不存在"）。
    let missing_parent = temp_dir("acc8-missing-root-parent");
    let missing_root = missing_parent.join("not-created-acceptance-root");
    assert!(
        !missing_root.exists(),
        "前置：隔离根子路径确实不存在（本次创建的临时父目录之下）"
    );
    let ctx_missing_root = DiscoveryContext {
        claude_default_root: outside_default.join("projects"),
        env: StartupEnv {
            codex_home: None,
            claude_config_dir: None,
            acceptance_isolated: true,
        },
        acceptance_root: Some(missing_root),
        ..ctx_for(&home, StartupEnv::default(), counting_probe.clone())
    };
    let out_missing = resolve_sources(
        &ctx_missing_root,
        &default_claude(),
        &default_codex(),
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    assert!(
        out_missing
            .roots
            .iter()
            .all(|r| !r.effective_enabled && r.state == ProbeState::Missing),
        "根不存在：全部候选停用且不探测：{:?}",
        out_missing.roots
    );

    // 3) 无法解析的根（隔离标志置位但 canonicalize 失败）：fail closed。
    //    用断链 junction 作根。
    let broken_root_link = home.join("broken-root-link");
    let dangling_target = home.join("gone-root");
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&dangling_target, &broken_root_link).unwrap();
    #[cfg(not(windows))]
    std::os::unix::fs::symlink(&dangling_target, &broken_root_link).unwrap();
    assert!(std::fs::canonicalize(&broken_root_link).is_err());
    let ctx_broken_root = DiscoveryContext {
        claude_default_root: outside_default.join("projects"),
        env: StartupEnv {
            codex_home: None,
            claude_config_dir: None,
            acceptance_isolated: true,
        },
        acceptance_root: Some(broken_root_link),
        ..ctx_for(&home, StartupEnv::default(), counting_probe.clone())
    };
    let out_broken = resolve_sources(
        &ctx_broken_root,
        &default_claude(),
        &default_codex(),
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    assert!(
        out_broken
            .roots
            .iter()
            .all(|r| !r.effective_enabled && r.state == ProbeState::Missing),
        "根无法解析：全部候选停用且不探测：{:?}",
        out_broken.roots
    );

    // 4) 隔离根内正例：默认根在根内 → 正常探测与启用
    let root = temp_dir("acc8-root");
    std::fs::create_dir_all(root.join("sources/claude")).unwrap();
    let ctx_inside = DiscoveryContext {
        claude_default_root: root.join("sources/claude"),
        env: StartupEnv {
            codex_home: None,
            claude_config_dir: None,
            acceptance_isolated: true,
        },
        acceptance_root: Some(root.clone()),
        ..ctx_for(&home, StartupEnv::default(), counting_probe.clone())
    };
    let out_inside = resolve_sources(
        &ctx_inside,
        &default_claude(),
        &SourceConfig::default(),
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    let inside_row = out_inside
        .roots
        .iter()
        .find(|r| r.agent == AgentKind::ClaudeCode)
        .expect("隔离根内默认行在场");
    assert!(
        inside_row.effective_enabled && inside_row.state == ProbeState::Ready,
        "隔离根内默认根正常探测与启用（目录已创建 → Ready）：{:?}",
        inside_row
    );

    // 5) 普通产品模式对照：isolated=false、根 None → 语义原样保留（根外默认
    //    根正常启用并探测）。
    let probed_before = probed.lock().unwrap().len();
    let ctx_plain = DiscoveryContext {
        claude_default_root: outside_default.join("projects"),
        env: StartupEnv {
            codex_home: None,
            claude_config_dir: None,
            acceptance_isolated: false,
        },
        acceptance_root: None,
        ..ctx_for(&home, StartupEnv::default(), counting_probe)
    };
    let out_plain = resolve_sources(
        &ctx_plain,
        &default_claude(),
        &SourceConfig::default(),
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    let plain_row = out_plain
        .roots
        .iter()
        .find(|r| r.agent == AgentKind::ClaudeCode)
        .expect("普通模式 Claude 行在场");
    assert!(
        plain_row.effective_enabled,
        "普通模式默认根语义不变：{:?}",
        plain_row
    );
    assert!(
        probed.lock().unwrap().len() > probed_before,
        "普通模式照常探测"
    );

    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&outside_default);
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&missing_parent);
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
            acceptance_isolated: false,
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
            acceptance_isolated: false,
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

// ── 符号链接能力豁免判定（最小测试：注入错误与 CI 形态驱动纯函数，
//    不改进程环境、不改系统设置）──────────────────────────────────────────

/// 原始码 1314 + CI 未设置：本地进入豁免（返回"能力不可用"，打印标记）。
#[test]
#[cfg(windows)]
fn symlink_exemption_allows_confirmed_privilege_error_locally() {
    let e = std::io::Error::from_raw_os_error(1314);
    assert!(
        !symlink_exemption_decision(&e, None),
        "确证的 1314 + 无 CI → 本地豁免：返回能力不可用（跳过并打印标记），\
         未执行项不冒充通过"
    );
}

/// 普通访问拒绝（原始码 5，非 1314）必须失败，不得归为"缺符号链接特权"。
#[test]
#[cfg(windows)]
#[should_panic(expected = "非符号链接特权问题")]
fn symlink_exemption_rejects_plain_permission_denied() {
    let e = std::io::Error::from_raw_os_error(5);
    let _ = symlink_exemption_decision(&e, None);
}

/// 其他任意错误（磁盘、竞态等）必须失败——即使 CI 形态也一样。
#[test]
#[cfg(windows)]
#[should_panic(expected = "非符号链接特权问题")]
fn symlink_exemption_rejects_other_errors() {
    let e = std::io::Error::other("disk full");
    let _ = symlink_exemption_decision(&e, Some(std::ffi::OsStr::new("1")));
}

/// CI 形态（CI 已设置）下确证的 1314 也必须失败——runner 必须真实构造断言。
#[test]
#[cfg(windows)]
#[should_panic(expected = "CI 环境缺少符号链接创建特权")]
fn symlink_exemption_fails_in_ci_without_privilege() {
    let e = std::io::Error::from_raw_os_error(1314);
    let _ = symlink_exemption_decision(&e, Some(std::ffi::OsStr::new("1")));
}

/// 探针的错误传递路径：创建失败时保留**原始创建错误**为主，不被清理结果覆盖。
/// 注入方式：目标目录不存在 → symlink_dir 必然失败，全程只触碰测试自建路径。
#[test]
#[cfg(windows)]
fn symlink_probe_preserves_creation_error_over_cleanup() {
    use std::os::windows::fs::symlink_dir;
    let workspace = temp_dir("symlink-probe-inject");
    let target = workspace.join("target");
    std::fs::create_dir_all(&target).unwrap();
    let link = workspace.join("link");
    std::fs::remove_dir(&target).unwrap();
    let created = symlink_dir(&target, &link);
    assert!(created.is_err(), "前置：目标不存在 → 创建失败");
    // 模拟探针的清理路径（清理自建 workspace）：错误对象按探针语义原样返回。
    std::fs::remove_dir_all(&workspace).unwrap();
    let preserved = created.unwrap_err();
    assert_ne!(
        preserved.raw_os_error(),
        Some(0),
        "原始创建错误保留，不被清理结果覆盖"
    );
}
