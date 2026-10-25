//! PR3 S03（计划 §6/§9.4/§8.3）：防环、多根与去重的采集契约。
//!
//! 用例名与 §8.3 事实矩阵逐项对应；统计类断言总量与请求身份集合，去重类
//! 再断言解析次数（`files_scanned`/`source_files` 登记），诊断类断言诊断。
//! 全部使用合成来源与隔离数据目录（cache/history/pricing/idx 都注入临时
//! 路径），不读写真实用户历史；Windows junction 只在临时根创建，清理先
//! 解除链接再删树。
//!
//! 关闭 S01 遗留：`upgrade_old_dir_plus_default_root_union` 执行完整的
//! 升级用量并集断言（配置侧已在 `source_paths_settings.rs` 固化）。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use tokenscope::model::AgentKind;
use tokenscope::report::{SourcePlan, SummaryOptions, summary};
use tokenscope::settings::{AdditionalSourcePath, SourceConfig};
use tokenscope::source::claude_projects::ProjectMapping;
use tokenscope::source::discovery::{DiscoveryContext, RootProbe, StartupEnv, resolve_sources};

static SEQ: AtomicU64 = AtomicU64::new(0);

fn temp_dir(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "tokenscope-sp-collection-{tag}-{}-{n}",
        std::process::id(),
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// 写一个 Claude 助手事件（`session`+`msg` 共同构成原生请求身份）。
fn write_claude_event(
    root: &Path,
    rel_dir: &str,
    file: &str,
    session: &str,
    msg: &str,
    input: u64,
) {
    let dir = root.join(rel_dir);
    std::fs::create_dir_all(&dir).unwrap();
    let body = format!(
        r#"{{"type":"assistant","timestamp":"2026-08-01T10:00:00.000Z","sessionId":"{session}","cwd":"C:/w/{rel_dir}","message":{{"id":"{msg}","model":"claude-sonnet-4-5","usage":{{"input_tokens":{input},"output_tokens":10}}}}}}"#
    );
    let path = dir.join(format!("{file}.jsonl"));
    if path.exists() {
        let mut text = std::fs::read_to_string(&path).unwrap();
        text.push_str(&body);
        text.push('\n');
        std::fs::write(&path, text).unwrap();
    } else {
        std::fs::write(&path, body + "\n").unwrap();
    }
}

/// 写一个合法的 Codex rollout 文件（1 个请求；`session` 进指纹身份）。
fn write_codex_event(root: &Path, rel_dir: &str, file: &str, session: &str, input: u64) {
    let day = root.join(rel_dir).join("2026").join("08").join("01");
    std::fs::create_dir_all(&day).unwrap();
    let body = format!(
        concat!(
            r#"{{"timestamp":"2026-08-01T10:00:00.000Z","type":"session_meta","payload":{{"id":"{session}","session_id":"{session}","cwd":"C:/w/codex-{rel_dir}"}}}}"#,
            "\n",
            r#"{{"timestamp":"2026-08-01T10:01:00.000Z","type":"turn_context","payload":{{"turn_id":"t1","model":"gpt-5.6-sol","cwd":"C:/w/codex-{rel_dir}"}}}}"#,
            "\n",
            r#"{{"timestamp":"2026-08-01T10:02:00.000Z","type":"event_msg","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":{input},"output_tokens":10,"cached_input_tokens":0,"cache_write_input_tokens":0,"reasoning_output_tokens":0,"total_tokens":{total}}}}}}}}}"#,
            "\n",
        ),
        session = session,
        rel_dir = rel_dir,
        input = input,
        total = input + 10,
    );
    std::fs::write(day.join(format!("rollout-{file}.jsonl")), body).unwrap();
}

/// 写一个**不带 cwd** 的 Claude 事件：项目身份走 slug 回退（映射测试用）。
fn write_claude_slug_event(
    root: &Path,
    slug_dir: &str,
    file: &str,
    session: &str,
    msg: &str,
    input: u64,
) {
    let dir = root.join(slug_dir);
    std::fs::create_dir_all(&dir).unwrap();
    let body = format!(
        r#"{{"type":"assistant","timestamp":"2026-08-01T10:00:00.000Z","sessionId":"{session}","message":{{"id":"{msg}","model":"claude-sonnet-4-5","usage":{{"input_tokens":{input},"output_tokens":10}}}}}}"#
    );
    std::fs::write(
        dir.join(format!("{file}.jsonl")),
        body + "
",
    )
    .unwrap();
}

fn claude_plan(root: &Path, mapping: Option<&Path>) -> SourcePlan {
    SourcePlan {
        agent: AgentKind::ClaudeCode,
        root: root.to_path_buf(),
        claude_mapping: mapping.map(|p| p.to_path_buf()),
    }
}

fn codex_plan(root: &Path) -> SourcePlan {
    SourcePlan {
        agent: AgentKind::Codex,
        root: root.to_path_buf(),
        claude_mapping: None,
    }
}

/// 密闭基础选项：所有路径都指向本次临时目录；计划由用例注入。
fn base_opts(dir: &Path, plans: Vec<SourcePlan>) -> SummaryOptions {
    SummaryOptions {
        workbuddy_enabled: Some(false),
        workbuddy_ai_enabled: Some(false),

        by: tokenscope::aggregate::GroupBy::Project,
        source_plans: Some(plans),
        cache_dir: Some(dir.join("cache")),
        history_path: Some(dir.join("history.db")),
        pricing_index: Some(dir.join("idx.json")),
        pricing_path: Some(dir.join("no-pricing.toml")),
        openrouter_path: Some(PathBuf::from("Z:/no-such/or.json")),
        modelsdev_path: Some(PathBuf::from("Z:/no-such/md.json")),
        tz: Some("Asia/Shanghai".to_string()),
        ..Default::default()
    }
}

/// 写一个 `.claude.json` 形态的映射：键 = 项目绝对路径（slug 由键编码得到）。
fn make_mapping(dir: &Path, name: &str, project_path: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(
        &path,
        format!(r#"{{"projects":{{"{project_path}":{{}}}}}}"#),
    )
    .unwrap();
    path
}

#[cfg(windows)]
fn make_junction(link: &Path, target: &Path) {
    let _ = std::fs::remove_file(link);
    let out = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()
        .expect("启动 mklink");
    assert!(
        out.status.success(),
        "创建 junction 失败（本环境必须支持，skip 不算通过）: {:?} / {:?}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[cfg(windows)]
fn remove_junction(link: &Path) {
    // 只解除链接本身，不沿链接清理目标
    let _ = std::fs::remove_file(link);
}

// ── §4.2：祖先环、深目录、断链 ──────────────────────────────────

/// 祖先联接环：`root/loop → root` 与 `root/A/self → A` 都在受控时间内终止，
/// 可读用量完整且只解析一次（修复前旧 walk 会无界递归）。
#[test]
#[cfg(windows)]
fn ancestor_junction_loops_terminate_with_complete_single_parse() {
    let dir = temp_dir("junction-loop");
    let root = dir.join("root");
    write_claude_event(&root, "A", "a", "s1", "m1", 100);
    make_junction(&root.join("loop"), &root);
    make_junction(&root.join("A").join("self"), &root.join("A"));

    let o = base_opts(&dir, vec![claude_plan(&root, None)]);
    let r = summary(&o).unwrap();
    assert_eq!(r.totals.requests, 1, "环内文件只解析一次: {:?}", r.totals);
    assert_eq!(r.totals.tokens.input, 100);
    assert!(
        !r.warnings.iter().any(|w| w.contains("归属冲突")),
        "环不是冲突: {:?}",
        r.warnings
    );
    remove_junction(&root.join("loop"));
    remove_junction(&root.join("A").join("self"));
    std::fs::remove_dir_all(&dir).ok();
}

/// 深目录（显式栈遍历）、失效联接（目标已删）与非 jsonl 文件：受控终止、
/// 可读用量完整、断链有诊断。
#[test]
#[cfg(windows)]
fn deep_tree_broken_junction_and_stray_files_terminate_with_diagnostics() {
    let dir = temp_dir("deep-broken");
    let root = dir.join("root");
    let mut deep = root.clone();
    for i in 0..120 {
        deep = deep.join(format!("d{i}"));
    }
    std::fs::create_dir_all(&deep).unwrap();
    write_claude_event(&deep, "", "bottom", "s-deep", "m-deep", 50);
    // 失效联接：目标目录随后删除
    let gone = dir.join("gone-target");
    std::fs::create_dir_all(&gone).unwrap();
    make_junction(&root.join("dangling"), &gone);
    std::fs::remove_dir_all(&gone).unwrap();
    // 非 jsonl 文件不入发现
    std::fs::write(root.join("notes.txt"), "not a log").unwrap();

    let o = base_opts(&dir, vec![claude_plan(&root, None)]);
    let r = summary(&o).unwrap();
    assert_eq!(r.totals.requests, 1, "深目录文件完整读取");
    assert!(
        r.warnings.iter().any(|w| w.contains("失效链接")),
        "断链必须诊断: {:?}",
        r.warnings
    );
    remove_junction(&root.join("dangling"));
    std::fs::remove_dir_all(&dir).ok();
}

// ── §4.3：两层去重与稳定认领者 ──────────────────────────────────

/// 同应用嵌套根（父/子）与重复路径：文件由最小规范化根认领，整轮只解析一次。
#[test]
fn same_agent_nested_and_duplicate_roots_parse_once() {
    let dir = temp_dir("nested-claims");
    let parent = dir.join("parent");
    write_claude_event(&parent, "a", "top", "s1", "m1", 100);
    write_claude_event(&parent, "a/sub", "inner", "s2", "m2", 60);

    // 两种输入顺序都得到同一结果（§9.4 第 7 条：交换根输入顺序结果相同）
    let plans_a = vec![
        claude_plan(&parent, None),
        claude_plan(&parent.join("a"), None),
    ];
    let plans_b = plans_a.iter().rev().cloned().collect::<Vec<_>>();
    let r_a = summary(&base_opts(&dir.join("hist-a"), plans_a)).unwrap();
    let r_b = summary(&base_opts(&dir.join("hist-b"), plans_b)).unwrap();
    for r in [&r_a, &r_b] {
        assert_eq!(r.totals.requests, 2, "两个独立请求都在: {:?}", r.totals);
        // 去重类：解析次数 = 文件数（子根不重复解析）
        let claude = r
            .sources
            .iter()
            .find(|s| s.agent == AgentKind::ClaudeCode)
            .unwrap();
        assert_eq!(
            claude.stats.files_scanned, 2,
            "嵌套根只解析一次: {:?}",
            claude.stats
        );
    }
    // 大小写 + 尾分隔符别名根：同根合并，不多扫描
    let alias = {
        let mut p = parent.clone().into_os_string();
        p.push("\\");
        PathBuf::from(p).to_string_lossy().to_uppercase()
    };
    let plans_alias = vec![
        claude_plan(&parent, None),
        SourcePlan {
            agent: AgentKind::ClaudeCode,
            root: PathBuf::from(alias),
            claude_mapping: None,
        },
    ];
    let r_c = summary(&base_opts(&dir.join("hist-c"), plans_alias)).unwrap();
    assert_eq!(r_c.totals.requests, 2);
    let claude = r_c
        .sources
        .iter()
        .find(|s| s.agent == AgentKind::ClaudeCode)
        .unwrap();
    assert_eq!(
        claude.stats.files_scanned, 2,
        "别名根不重复解析: {:?}",
        claude.stats
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// 物理独立复制：两份都解析，事实层按原生身份求并集，统计不增量；
/// 独立原生身份不丢（§8.3「物理独立复制一份」）。
#[test]
fn copied_files_parse_but_native_identity_dedupes_without_loss() {
    let dir = temp_dir("copy-union");
    let root = dir.join("root");
    write_claude_event(&root, "p1", "orig", "s1", "m1", 100);
    // 同一原生身份的副本（不同文件名、不同子目录）
    write_claude_event(&root, "p2", "copy", "s1", "m1", 100);
    // 独立请求（不同 message id）
    write_claude_event(&root, "p3", "other", "s2", "m2", 70);

    let o = base_opts(&dir, vec![claude_plan(&root, None)]);
    let r = summary(&o).unwrap();
    let claude = r
        .sources
        .iter()
        .find(|s| s.agent == AgentKind::ClaudeCode)
        .unwrap();
    assert_eq!(claude.stats.files_scanned, 3, "三份物理文件都解析");
    assert_eq!(r.totals.requests, 2, "副本按原生身份求并集，独立请求不丢");
    assert_eq!(r.totals.tokens.input, 170, "100 + 70，副本不增量");
    std::fs::remove_dir_all(&dir).ok();
}

/// 硬链接（同一物理文件的两个路径）：按 §0 第 1 条不做物理身份去重——
/// 两个路径都解析一次，事实层按原生身份求并集，统计不增量。
#[test]
fn hard_link_paths_each_parse_once_and_union_in_history() {
    let dir = temp_dir("hardlink");
    let root = dir.join("root");
    write_claude_event(&root, "p1", "orig", "s1", "m1", 100);
    std::fs::hard_link(root.join("p1/orig.jsonl"), root.join("p1/hard.jsonl")).unwrap();

    let o = base_opts(&dir, vec![claude_plan(&root, None)]);
    let r = summary(&o).unwrap();
    let claude = r
        .sources
        .iter()
        .find(|s| s.agent == AgentKind::ClaudeCode)
        .unwrap();
    assert_eq!(
        claude.stats.files_scanned, 2,
        "两个路径各解析一次（不做文件 ID 去重）"
    );
    assert_eq!(r.totals.requests, 1, "事实层求并集，统计不增量");
    std::fs::remove_dir_all(&dir).ok();
}

// ── §9.4：映射与归属 ────────────────────────────────────────────

/// 映射上下文改变必须重解析（§9.4 第 6 条）：热轮零重解析，映射内容修订后
/// 同一文件重新解析一次（归属 slug 由键编码得到，保持解析正确）。
#[test]
fn mapping_context_change_reparses_file() {
    let dir = temp_dir("mapping-change");
    let root = dir.join("root");
    // slug 目录名 = slug_for_path("C:/w/alpha")；事件不带 cwd 走 slug 回退
    write_claude_slug_event(&root, "C--w-alpha", "a", "s1", "m1", 100);
    let mapping = make_mapping(&dir, "claude.json", "C:/w/alpha");

    let o = base_opts(&dir, vec![claude_plan(&root, Some(&mapping))]);
    let r1 = summary(&o).unwrap();
    let claude1 = r1
        .sources
        .iter()
        .find(|s| s.agent == AgentKind::ClaudeCode)
        .unwrap();
    assert_eq!(claude1.stats.files_scanned, 1);
    assert!(
        r1.groups.iter().any(|g| g.key.contains("C:/w/alpha")),
        "映射命中: {:?}",
        r1.groups
    );

    // 重复采集：统计不增量（无新增事实）
    let r2 = summary(&o).unwrap();
    assert_eq!(r2.totals.requests, 1);

    // 映射内容修订（新增无关条目）→ revision 变化 → 采集键变化 → 必须真实
    // 重解析一次（不依赖复用槽位，确定性），归属不变
    std::fs::write(
        &mapping,
        r#"{"projects":{"C:/w/alpha":{},"C:/w/other":{}}}"#,
    )
    .unwrap();
    let r3 = summary(&o).unwrap();
    let claude3 = r3
        .sources
        .iter()
        .find(|s| s.agent == AgentKind::ClaudeCode)
        .unwrap();
    assert_eq!(
        claude3.stats.files_scanned, 1,
        "映射修订必须让已登记指纹失效并重解析"
    );
    assert!(
        r3.groups.iter().any(|g| g.key.contains("C:/w/alpha")),
        "重解析后归属保持正确"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// §8.3「默认与环境同 slug、各自映射不同」：默认根走已核实映射，环境根
/// （映射未核实被禁用）退回 slug 身份——同名 slug 不串映射；手动根不借映射。
#[test]
fn same_slug_across_roots_do_not_cross_mappings() {
    let dir = temp_dir("slug-cross");
    let default_root = dir.join("default");
    let env_root = dir.join("env");
    // 同名 slug 目录（slug_for_path("C:/w/alpha") 的编码形态）、不同会话与消息；
    // 事件不带 cwd → slug 回退，映射只影响绑定了它的根
    write_claude_slug_event(&default_root, "C--w-alpha", "a", "s-def", "m-def", 100);
    write_claude_slug_event(&env_root, "C--w-alpha", "b", "s-env", "m-env", 60);
    let mapping = make_mapping(&dir, "claude.json", "C:/w/alpha");

    // 发现层的绑定结论：默认根带映射、环境根禁用（S02）——计划按此注入
    let plans = vec![
        claude_plan(&default_root, Some(&mapping)),
        claude_plan(&env_root, None),
    ];
    let o = base_opts(&dir, plans);
    let r = summary(&o).unwrap();
    assert_eq!(r.totals.requests, 2);
    let mapped = r.groups.iter().any(|g| g.key.contains("C:/w/alpha"));
    let slug_kept = r.groups.iter().any(|g| g.key.ends_with("alpha"));
    assert!(mapped, "默认根经映射归属: {:?}", r.groups);
    assert!(
        slug_kept,
        "环境根退回 slug 身份（C--w-alpha），不借用默认映射: {:?}",
        r.groups
    );
    std::fs::remove_dir_all(&dir).ok();
}

// ── §4.1：跨应用隔离 ────────────────────────────────────────────

/// §8.3「两个 adapter 认领同一根」：后到根隔离 + 诊断，其余根继续；
/// 不按扫描顺序选择（claude 恒先于 codex 是排序结果，非发现顺序运气）。
/// 跨应用根冲突（§4.1/§8.5，审阅 §23.2 补修）：冲突判定在**发现层**整组
/// 隔离——不同应用的有效根相同或互为父子时，双方行都标记冲突并写明对方，
/// 计划排除后双方都不读取；其余根照常。此前实现共用遍历台账且 Claude 恒在
/// Codex 之前，跨应用重叠实际是"先到者获胜"（后到应用被台账挡下空手而归，
/// 它的文件由先到应用的解析器读取），本用例按修复后的语义断言。
///
/// 走 `resolve_sources` →（按壳侧同规则过滤冲突行）→ `summary`，不做只
/// 断言采集层兜底台账的"先到者获胜"快照。返回采集诊断与总请求数。
fn cross_app_conflict_case(
    tag: &str,
    claude_paths: &[&Path],
    codex_paths: &[&Path],
    expect_conflicted: &[&Path],
) -> (Vec<String>, u64) {
    let dir = temp_dir(tag);
    let home = dir.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let probe = |p: &Path| RootProbe {
        state: if p.exists() {
            tokenscope::source::discovery::ProbeState::Ready
        } else {
            tokenscope::source::discovery::ProbeState::Missing
        },
        canonical: p.canonicalize().ok(),
    };
    let ctx = DiscoveryContext::for_test(home, StartupEnv::default(), std::sync::Arc::new(probe));
    let manual = |paths: &[&Path]| SourceConfig {
        enabled: true,
        dir: None,
        auto_discover: false,
        excluded_auto_paths: Vec::new(),
        additional_paths: paths
            .iter()
            .map(|p| AdditionalSourcePath {
                path: p.display().to_string(),
                enabled: true,
            })
            .collect(),
        legacy_format: false,
    };
    let resolved = resolve_sources(
        &ctx,
        &manual(claude_paths),
        &manual(codex_paths),
        &SourceConfig::default(),
        &SourceConfig::default(),
    );
    // 发现层断言：冲突标记恰好覆盖重叠集合，且冲突行诊断写明对方与处置。
    let expected: Vec<String> = expect_conflicted.iter().map(|p| normalized(p)).collect();
    for row in &resolved.roots {
        let is_overlapped = expected.contains(&normalized(Path::new(&row.display_path)));
        assert_eq!(
            row.conflict_with.is_some(),
            is_overlapped,
            "冲突标记应恰好覆盖重叠根: {row:?}"
        );
        if is_overlapped {
            assert!(
                row.diagnostics
                    .iter()
                    .any(|d| d.contains("冲突") && d.contains("本轮双方都不读取")),
                "冲突行必须有诊断: {:?}",
                row.diagnostics
            );
        }
    }
    // 按壳侧同规则把冲突行排除出计划（source_plans_from_with 的语义），
    // 再走真实采集：冲突根的文件谁都不读。
    let plans: Vec<SourcePlan> = resolved
        .roots
        .iter()
        .filter(|row| row.effective_enabled && row.conflict_with.is_none())
        .map(|row| SourcePlan {
            agent: row.agent,
            root: PathBuf::from(&row.display_path),
            claude_mapping: row.mapping_path.clone(),
        })
        .collect();
    let o = base_opts(&dir, plans);
    let r = summary(&o).unwrap();
    std::fs::remove_dir_all(&dir).ok();
    (r.warnings, r.totals.requests)
}

fn normalized(p: &Path) -> String {
    tokenscope::settings::normalized_key(p)
}

#[test]
fn cross_app_same_root_both_sides_conflicted_and_excluded() {
    let dir = temp_dir("cross-app-root");
    let shared = dir.join("shared");
    write_claude_event(&shared, "", "c", "s-cl", "m-cl", 100);
    write_codex_event(&shared, "", "x", "s-cx", 60);
    let codex_own = dir.join("codex-own");
    write_codex_event(&codex_own, "", "y", "s-cy", 30);

    let (warnings, requests) = cross_app_conflict_case(
        "conflict-same",
        &[&shared],
        &[&shared, &codex_own],
        &[&shared],
    );
    // 只有 Codex 独立根的 1 条请求计入：共享根里的 Claude/Codex 事件谁都不读。
    assert_eq!(requests, 1, "冲突根双方都不读取: {requests}");
    assert!(
        !warnings.iter().any(|w| w.contains("跨应用")),
        "冲突在发现层解决，采集层不应再出现跨应用兜底诊断: {warnings:?}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn cross_app_nested_claude_parent_codex_child_both_excluded() {
    let dir = temp_dir("cross-app-nest-a");
    let parent = dir.join("parent");
    write_claude_event(&parent, "", "c", "s-cl", "m-cl", 100);
    let child = parent.join("sub");
    write_codex_event(&child, "", "x", "s-cx", 60);
    let codex_own = dir.join("codex-own");
    write_codex_event(&codex_own, "", "y", "s-cy", 30);

    let (_warnings, requests) = cross_app_conflict_case(
        "conflict-nest-a",
        &[&parent],
        &[&child, &codex_own],
        &[&parent, &child],
    );
    // 嵌套冲突双方（Claude 父 / Codex 子）都不读取，只有 Codex 独立根的
    // 1 条请求计入。
    assert_eq!(requests, 1, "嵌套冲突双方都不读取: {requests}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn cross_app_nested_codex_parent_claude_child_both_excluded() {
    let dir = temp_dir("cross-app-nest-b");
    let parent = dir.join("parent");
    write_codex_event(&parent, "", "x", "s-cx", 60);
    let child = parent.join("sub");
    write_claude_event(&child, "", "c", "s-cl", "m-cl", 100);
    let codex_own = dir.join("codex-own");
    write_codex_event(&codex_own, "", "y", "s-cy", 30);

    let (_warnings, requests) = cross_app_conflict_case(
        "conflict-nest-b",
        &[&child],
        &[&parent, &codex_own],
        &[&parent, &child],
    );
    assert_eq!(requests, 1, "反方向嵌套冲突双方都不读取: {requests}");
    std::fs::remove_dir_all(&dir).ok();
}

// ── §8.3：升级用量并集（关闭 S01 遗留）──────────────────────────

/// §6 S01/S03 升级用例（完整用量断言）：旧配置只读自定义 dir；迁移后
/// （计划 = 默认根 + 自定义根）默认根独立用量并入、重复请求不增量、
/// 重复采集不变、停用不读取且历史保留、恢复后新事件纳入。
#[test]
fn upgrade_old_dir_plus_default_root_union() {
    let dir = temp_dir("upgrade-union");
    let default_root = dir.join("home").join(".claude").join("projects");
    let custom_root = dir.join("custom");
    // 自定义根：请求 A；默认根：请求 B + A 的副本（不同文件、同原生身份）
    write_claude_event(&custom_root, "pA", "a1", "sA", "mA", 100);
    write_claude_event(&default_root, "pB", "b1", "sB", "mB", 60);
    write_claude_event(&default_root, "pB2", "b2", "sA", "mA", 100);

    let migrated = vec![
        claude_plan(&default_root, None),
        claude_plan(&custom_root, None),
    ];
    let o = base_opts(&dir, migrated);
    let first = tokenscope::report::collect_only(&o).unwrap();
    let r1 = summary(&o).unwrap();
    assert_eq!(r1.totals.requests, 2, "升级后恰为 A∪B: {:?}", r1.totals);
    assert_eq!(r1.totals.tokens.input, 160, "100 + 60，副本不增量");
    let claude = r1
        .sources
        .iter()
        .find(|s| s.agent == AgentKind::ClaudeCode)
        .unwrap();
    assert_eq!(claude.stats.files_scanned, 3, "三份物理文件都解析");

    // 重复迁移/采集结果不变：无论走复用快照还是真实重扫，统计都不增量
    let hot = tokenscope::report::collect_only(&o).unwrap();
    assert_eq!(
        hot.generation, first.generation,
        "热轮不推进 generation（无新增事实不写入）"
    );
    let r2 = summary(&o).unwrap();
    assert_eq!(r2.totals.requests, 2);

    // 停用自定义根（计划只剩默认根）：新写入自定义根的事件不读取；
    // 已保存事实仍在（查询仍为 2）。
    let disabled = vec![claude_plan(&default_root, None)];
    write_claude_event(&custom_root, "pA", "a2", "sC", "mC", 30);
    let o_disabled = base_opts(&dir, disabled);
    let r3 = summary(&o_disabled).unwrap();
    assert_eq!(r3.totals.requests, 2, "停用来源后新事件不纳入，历史保留");

    // 恢复后（迁移语义）新事件纳入。
    write_claude_event(&default_root, "pB", "b3", "sD", "mD", 20);
    let r4 = summary(&o).unwrap();
    assert_eq!(r4.totals.requests, 4, "A∪B∪C∪D");
    assert_eq!(r4.totals.tokens.input, 210);
    std::fs::remove_dir_all(&dir).ok();
}

/// §2 行为变更「CODEX_HOME 追加默认根」：旧版只读环境根，新版默认与环境
/// 都读——升级用量取并集；同会话副本经指纹求并集不增量（§9.4 第 7 条）。
#[test]
fn codex_home_upgrade_reads_default_and_env_roots() {
    let dir = temp_dir("codex-upgrade");
    let default_root = dir.join("home").join(".codex").join("sessions");
    let env_root = dir.join("portable").join("sessions");
    // 默认根：会话 X；环境根：会话 Y + X 的副本（同 session/model/四桶）
    write_codex_event(&default_root, "", "dx", "sX", 100);
    write_codex_event(&env_root, "", "ey", "sY", 60);
    write_codex_event(&env_root, "", "ex-copy", "sX", 100);

    let old_style = vec![codex_plan(&env_root)];
    let r_old = summary(&base_opts(&dir.join("hist-old"), old_style)).unwrap();
    assert_eq!(r_old.totals.requests, 2, "旧语义只读环境根（X 副本去重）");

    let new_style = vec![codex_plan(&default_root), codex_plan(&env_root)];
    let r_new = summary(&base_opts(&dir.join("hist-new"), new_style)).unwrap();
    assert_eq!(
        r_new.totals.requests, 2,
        "升级后默认与环境都读，重复会话不增量"
    );
    assert_eq!(r_new.totals.tokens.input, 160);
    std::fs::remove_dir_all(&dir).ok();
}

// ── §8.3：停用/删除来源后重启、上下文变化 ───────────────────────

/// 文件截断后指纹变化必须重解析（不沿用旧登记），有效事实正确；
/// 历史事实按既有语义保留。
#[test]
fn truncated_file_reparses_with_fresh_fingerprint() {
    let dir = temp_dir("truncate");
    let root = dir.join("root");
    write_claude_event(&root, "p", "a", "s1", "m1", 100);
    write_claude_event(&root, "p", "a", "s2", "m2", 40);
    let file = root.join("p").join("a.jsonl");

    let o = base_opts(&dir, vec![claude_plan(&root, None)]);
    let r1 = summary(&o).unwrap();
    assert_eq!(r1.totals.requests, 2);

    // 截断（重写为只剩第一个事件）→ 指纹变化 → 本轮重解析该文件
    let text = std::fs::read_to_string(&file).unwrap();
    let first_line = text.lines().next().unwrap();
    std::fs::write(&file, first_line.to_owned() + "\n").unwrap();
    let r2 = summary(&o).unwrap();
    let claude = r2
        .sources
        .iter()
        .find(|s| s.agent == AgentKind::ClaudeCode)
        .unwrap();
    assert_eq!(
        claude.stats.files_scanned, 1,
        "截断后必须重解析（不沿用错误登记）"
    );
    assert_eq!(r2.totals.requests, 2, "已保存事实保留（含被截断掉的事件）");
    std::fs::remove_dir_all(&dir).ok();
}

/// 采集中变更来源集合只影响下一轮：每轮以**本轮传入的计划**为准
/// （冻结语义的壳侧接线在 S04；核心层断言计划变化 → 结果跟随计划）。
#[test]
fn root_set_change_between_rounds_changes_reading_scope() {
    let dir = temp_dir("set-change");
    let root_a = dir.join("a");
    let root_b = dir.join("b");
    write_claude_event(&root_a, "", "a", "sA", "mA", 100);
    write_claude_event(&root_b, "", "b", "sB", "mB", 60);

    let only_a = base_opts(&dir, vec![claude_plan(&root_a, None)]);
    let r1 = summary(&only_a).unwrap();
    assert_eq!(r1.totals.requests, 1);

    // 追加根 B：下一轮并入
    let both = base_opts(
        &dir,
        vec![claude_plan(&root_a, None), claude_plan(&root_b, None)],
    );
    let r2 = summary(&both).unwrap();
    assert_eq!(r2.totals.requests, 2, "新根下一轮出现并被读取");

    // 移除根 A：不再读取（新追加事件不纳入），历史保留
    write_claude_event(&root_a, "", "a2", "sA2", "mA2", 10);
    let only_b = base_opts(&dir, vec![claude_plan(&root_b, None)]);
    let r3 = summary(&only_b).unwrap();
    assert_eq!(r3.totals.requests, 2, "移除的根不再读取，历史事实保留");
    std::fs::remove_dir_all(&dir).ok();
}

/// 计划为空（两应用全停）→ 零采集；历史不丢。
#[test]
fn empty_plan_set_collects_nothing_and_keeps_history() {
    let dir = temp_dir("empty-plans");
    let root = dir.join("root");
    write_claude_event(&root, "", "a", "s1", "m1", 100);
    let o = base_opts(&dir, vec![claude_plan(&root, None)]);
    assert_eq!(summary(&o).unwrap().totals.requests, 1);

    let none = base_opts(&dir, Vec::new());
    let r = summary(&none).unwrap();
    assert_eq!(r.totals.requests, 1, "历史事实保留");
    assert!(r.sources.is_empty(), "无启用来源：零采集报告");
    std::fs::remove_dir_all(&dir).ok();
}

/// ProjectMapping 注入形态（计划 → with_mapping）与 disabled 形态等价可用。
#[test]
fn plan_mapping_binding_loads_and_resolves() {
    let dir = temp_dir("mapping-binding");
    let mapping = make_mapping(&dir, "claude.json", "C:/proj/x");
    let loaded = ProjectMapping::load_cached(&mapping);
    assert_eq!(loaded.resolve("C--proj-x"), Some("C:/proj/x"));
    let disabled = ProjectMapping::disabled();
    assert_eq!(disabled.resolve("C--proj-x"), None);
    std::fs::remove_dir_all(&dir).ok();
}

/// 共享冲突限于临时根；File 在正常结束或 panic 时自动释放句柄。
#[cfg(windows)]
fn hold_read_conflict(root: &Path, path: &Path, directory: bool) -> std::fs::File {
    use std::os::windows::fs::OpenOptionsExt;

    let root = root.canonicalize().unwrap();
    let path = path.canonicalize().unwrap();
    assert!(
        path.starts_with(&root) && path != root,
        "共享冲突必须局限于临时根子项"
    );
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x02000000;
    std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .custom_flags(if directory {
            FILE_FLAG_BACKUP_SEMANTICS
        } else {
            0
        })
        .open(&path)
        .unwrap_or_else(|error| panic!("持有读取冲突句柄失败 {}: {error}", path.display()))
}

#[test]
#[cfg(windows)]
fn unreadable_subdir_and_file_are_diagnosed_without_stopping_the_round() {
    let dir = temp_dir("read-conflict");
    let root = dir.join("sources");
    write_claude_event(&root, "readable", "ok-a", "s-ok-a", "m-ok-a", 10);
    write_claude_event(&root, "denied-dir", "hidden", "s-hidden", "m-hidden", 20);
    write_claude_event(&root, "mixed", "ok-b", "s-ok-b", "m-ok-b", 30);
    write_claude_event(&root, "mixed", "denied-file", "s-denied", "m-denied", 40);
    let denied_dir = root.join("denied-dir");
    let denied_file = root.join("mixed/denied-file.jsonl");
    let directory_guard = hold_read_conflict(&dir, &denied_dir, true);
    let file_guard = hold_read_conflict(&dir, &denied_file, false);

    let directory_failure = std::fs::read_dir(&denied_dir)
        .expect_err("持有独占目录句柄时 read_dir 必须失败；自检不允许跳过");
    let file_failure = std::fs::read_to_string(&denied_file)
        .expect_err("持有独占文件句柄时读取必须失败；自检不允许跳过");

    let mut opts = base_opts(&dir, vec![claude_plan(&root, None)]);
    opts.openrouter_path = Some(dir.join("no-openrouter.json"));
    opts.modelsdev_path = Some(dir.join("no-modelsdev.json"));
    let report = summary(&opts).expect("读取失败不得中断其余文件的采集与汇总");
    eprintln!(
        "AC01_OBSERVED {}",
        serde_json::json!({
            "directory_failure": {
                "kind": format!("{:?}", directory_failure.kind()),
                "raw_os_error": directory_failure.raw_os_error(),
                "message": directory_failure.to_string(),
            },
            "file_failure": {
                "kind": format!("{:?}", file_failure.kind()),
                "raw_os_error": file_failure.raw_os_error(),
                "message": file_failure.to_string(),
            },
            "requests": report.totals.requests,
            "tokens": report.totals.tokens,
            "io_errors": report.sources.first().map(|source| source.stats.io_errors),
            "warnings": report.warnings,
        })
    );
    assert_eq!(report.totals.requests, 2, "两个可读文件完整入账");
    assert_eq!(report.totals.tokens.input, 40);
    assert_eq!(report.totals.tokens.output, 20);
    assert_eq!(report.totals.tokens.cache_write, 0);
    assert_eq!(report.totals.tokens.cache_read, 0);
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("无法读取目录") && w.contains("denied-dir")),
        "目录拒绝必须有可见诊断: {:?}",
        report.warnings
    );
    assert_eq!(report.sources.len(), 1);
    assert_eq!(
        report.sources[0].stats.io_errors, 1,
        "文件拒绝计入 io_errors"
    );
    assert!(
        report.warnings.iter().any(|w| w.contains("读取失败")),
        "文件拒绝必须有可见诊断: {:?}",
        report.warnings
    );

    drop(file_guard);
    drop(directory_guard);
    assert!(std::fs::read_dir(&denied_dir).is_ok(), "目录句柄已释放");
    assert!(
        std::fs::read_to_string(&denied_file).is_ok(),
        "文件句柄已释放"
    );
    // summary 的查询快照按设计仍在注册表中持有只读事务；只回收本用例的
    // 会话后再删库，既不清空其他并发用例的会话，也不忽略 Windows 占用错误。
    tokenscope::query::backdate_query_for_tests(
        &report.query_id,
        tokenscope::query::QUERY_IDLE_TTL,
    );
    tokenscope::query::query_summary(&report.query_id).expect_err("本用例查询已回收");
    std::fs::remove_dir_all(&dir).unwrap();
}
