//! Task 2：来源目录重叠——Claude 与 Codex 指向同一规范化目录时，
//! 同一日志文件最多贡献一次统计；缓存不互相覆盖（配置与采集双层防御）。
//! 所有路径显式注入临时目录。

use std::path::PathBuf;

use std::sync::atomic::Ordering;
use tokenscope::aggregate::GroupBy;
use tokenscope::report::{SummaryOptions, cache_stats, summary};

fn tmp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "tokenscope-overlap-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// 共享日志目录：一个 Claude 会话文件（只含 claude 形态的行）。
fn shared_dir(dir: &std::path::Path) -> PathBuf {
    let shared = dir.join("shared");
    std::fs::create_dir_all(&shared).unwrap();
    std::fs::write(
        shared.join("s.jsonl"),
        r#"{"type":"assistant","timestamp":"2026-08-01T10:00:00.000Z","sessionId":"c1","message":{"id":"m","model":"claude-sonnet-4-5","usage":{"input_tokens":100,"output_tokens":10}}}"#,
    )
    .unwrap();
    shared
}

fn opts(
    dir: &std::path::Path,
    shared: &std::path::Path,
    codex_dir: Option<PathBuf>,
) -> SummaryOptions {
    SummaryOptions {
        workbuddy_enabled: Some(false),
        workbuddy_ai_enabled: Some(false),

        by: GroupBy::Day,
        claude_dir: Some(shared.to_path_buf()),
        codex_dir,
        cache_dir: Some(dir.join("cache")),
        pricing_index: Some(dir.join("idx.json")),
        pricing_path: Some(dir.join("no-pricing.toml")),
        openrouter_path: Some(PathBuf::from("Z:/no-such/openrouter-snapshot.json")),
        modelsdev_path: Some(PathBuf::from("Z:/no-such/modelsdev-snapshot.json")),
        tz: Some("Asia/Shanghai".to_string()),
        ..Default::default()
    }
}

/// PR3 §4.1/§8.3：跨应用来源根相同 → **隔离**该重叠根并诊断，其余照常
/// 读取（不再整体报错）；硬拒绝只发生在保存事务（S04 接线）。
#[test]
fn test_source_overlap_same_dir_single_count() {
    let dir = tmp_dir("same");
    let shared = shared_dir(&dir);
    let o = opts(&dir, &shared, Some(shared.to_path_buf()));
    let r = summary(&o).unwrap();
    assert_eq!(r.totals.requests, 1, "claude 照常读取，codex 根被隔离");
    assert!(
        r.warnings.iter().any(|w| w.contains("跨应用来源根重叠")),
        "隔离必须诊断可见: {:?}",
        r.warnings
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// PR3：尾分隔符别名指向同一规范化目录 → 同样隔离（台账按规范化键去重）。
#[test]
fn test_source_overlap_alias_paths_recognized() {
    let dir = tmp_dir("alias");
    let shared = shared_dir(&dir);
    let alias = {
        let mut p = shared.clone().into_os_string();
        p.push("\\"); // 尾部分隔符别名
        PathBuf::from(p)
    };
    let o = opts(&dir, &shared, Some(alias));
    let r = summary(&o).unwrap();
    assert_eq!(r.totals.requests, 1, "别名根隔离，claude 只计一份");
    assert!(r.warnings.iter().any(|w| w.contains("跨应用来源根重叠")));
    std::fs::remove_dir_all(&dir).ok();
}

/// PR3：跨应用嵌套（父/子）→ 子根被父根的遍历覆盖，隔离 + 诊断；
/// 父根文件完整读取一次（§4.1：不采用子根覆盖父根）。
#[test]
fn test_source_overlap_nested_dir_rejected() {
    let dir = tmp_dir("nested");
    let shared = shared_dir(&dir);
    let child = shared.join("sub");
    std::fs::create_dir_all(&child).unwrap();
    let o = opts(&dir, &shared, Some(child));
    let r = summary(&o).unwrap();
    assert_eq!(r.totals.requests, 1, "父根照常读取，子根隔离");
    assert!(r.warnings.iter().any(|w| w.contains("跨应用来源根重叠")));
    std::fs::remove_dir_all(&dir).ok();
}

/// PR3：大小写别名（Windows 不敏感语义）→ 同样隔离，不重复解析。
#[test]
fn test_source_overlap_case_alias_rejected() {
    let dir = tmp_dir("case");
    let shared = shared_dir(&dir);
    let upper: PathBuf = shared.to_string_lossy().to_uppercase().into();
    let o = opts(&dir, &shared, Some(upper));
    let r = summary(&o).unwrap();
    assert_eq!(r.totals.requests, 1, "大小写别名根隔离");
    assert!(r.warnings.iter().any(|w| w.contains("跨应用来源根重叠")));
    std::fs::remove_dir_all(&dir).ok();
}

/// SF09（保存校验侧，S04 接线时沿用同一函数）：显式目录撞**默认根**也在
/// 有效目录解析后识别为冲突——这里用纯校验函数核对，不对真实默认根跑采集。
#[test]
fn overlap_validation_uses_effective_default_paths() {
    let dir = tmp_dir("default-paths");
    // codex 显式目录 = Claude 的默认根 → 有效目录冲突。
    let claude_default = tokenscope::source::claude::ClaudeSource::default_root().unwrap();
    let o = SummaryOptions {
        workbuddy_enabled: Some(false),
        workbuddy_ai_enabled: Some(false),

        by: GroupBy::Day,
        claude_dir: None, // = Claude 默认根
        codex_dir: Some(claude_default.clone()),
        cache_dir: Some(dir.join("cache")),
        pricing_index: Some(dir.join("idx.json")),
        pricing_path: Some(dir.join("no-pricing.toml")),
        tz: Some("Asia/Shanghai".to_string()),
        ..Default::default()
    };
    let claude_effective = tokenscope::report::effective_source_roots(
        tokenscope::model::AgentKind::ClaudeCode,
        o.claude_enabled != Some(false),
        o.claude_dir.as_deref(),
    )
    .unwrap();
    assert_eq!(claude_effective, vec![claude_default.clone()]);
    let err = tokenscope::settings::validate_dir_conflict(
        &claude_effective[0],
        &o.codex_dir.clone().unwrap(),
    )
    .unwrap_err();
    assert!(
        err.contains("来源目录冲突"),
        "显式目录撞默认根必须识别: {err}"
    );
    // 分离目录照常采集（不触真实默认根）。
    let o2 = SummaryOptions {
        workbuddy_enabled: Some(false),
        workbuddy_ai_enabled: Some(false),

        by: GroupBy::Day,
        cache_dir: Some(dir.join("cache2")),
        pricing_index: Some(dir.join("idx2.json")),
        pricing_path: Some(dir.join("no-pricing.toml")),
        tz: Some("Asia/Shanghai".to_string()),
        ..Default::default()
    };
    // 不落真实目录内容：claude 默认根可能存在真实数据——改用显式隔离目录
    // 验证「默认根不冲突」的判定逻辑本身（两默认根不同 → 校验放行），
    // 采集侧用注入目录避免触碰真实 ~/.claude / ~/.codex。
    let _ = &o2;
    let shared2 = shared_dir(&dir);
    let o3 = SummaryOptions {
        workbuddy_enabled: Some(false),
        workbuddy_ai_enabled: Some(false),

        by: GroupBy::Day,
        claude_dir: Some(shared2.clone()),
        codex_dir: Some(dir.join("other-codex")),
        cache_dir: Some(dir.join("cache3")),
        pricing_index: Some(dir.join("idx3.json")),
        pricing_path: Some(dir.join("no-pricing.toml")),
        tz: Some("Asia/Shanghai".to_string()),
        ..Default::default()
    };
    let r = summary(&o3).unwrap();
    assert_eq!(r.totals.requests, 1, "无冲突配置照常采集");
    std::fs::remove_dir_all(&dir).ok();
}

/// SF09：停用其一来源即可恢复——冲突校验只对**启用**来源生效。
#[test]
fn disabled_source_allows_overlap_recovery() {
    let dir = tmp_dir("disabled");
    let shared = shared_dir(&dir);
    let o = opts(&dir, &shared, Some(shared.to_path_buf()));
    let mut o = o;
    o.codex_enabled = Some(false);
    let r = summary(&o).unwrap();
    assert_eq!(r.totals.requests, 1, "停用冲突来源后配置可恢复");
    std::fs::remove_dir_all(&dir).ok();
}

/// AP01：缺省字段（`None`）= 默认启用 + 工具默认根，因此在有效来源解析里
/// **同样参与**冲突判定。修复前保存校验把 `None` 当成"不参与校验"，允许
/// 保存一份采集层必然拒绝的配置（保存成功 → 之后每次查询报冲突）。
#[test]
fn default_enabled_source_participates_in_overlap_validation() {
    use tokenscope::model::AgentKind;
    use tokenscope::report::effective_source_dirs_from_settings;
    use tokenscope::settings::{AgentSources, Settings, SourceConfig};

    let claude_default = tokenscope::source::claude::ClaudeSource::default_root().unwrap();
    let codex_default = tokenscope::source::codex::CodexSource::default_root().unwrap();
    let codex_archived = codex_default.parent().unwrap().join("archived_sessions");

    // 全缺省配置：全部来源都解析出来——"字段缺失 = 默认启用 + 默认根"。
    // H02：Codex 默认来源含 sessions 与 archived_sessions **两个根**，
    // 两者都必须参与冲突校验。
    // WB04 产品口径变化：候选集从 3 根扩为 5 根（+ WorkBuddy 2 根）。
    let wb_default =
        tokenscope::source::workbuddy::WorkBuddySource::default_root(AgentKind::WorkBuddy).unwrap();
    let wba_default =
        tokenscope::source::workbuddy::WorkBuddySource::default_root(AgentKind::WorkBuddyAi)
            .unwrap();
    let dirs = effective_source_dirs_from_settings(&Settings::default()).unwrap();
    assert_eq!(dirs.len(), 5, "缺省字段必须解析为默认启用: {dirs:?}");
    assert_eq!(dirs[0], (AgentKind::ClaudeCode, claude_default.clone()));
    assert_eq!(dirs[1], (AgentKind::Codex, codex_default.clone()));
    assert_eq!(dirs[2], (AgentKind::Codex, codex_archived.clone()));
    assert_eq!(dirs[3], (AgentKind::WorkBuddy, wb_default.clone()));
    assert_eq!(dirs[4], (AgentKind::WorkBuddyAi, wba_default.clone()));

    // 显式停用 → 不参与（用户可借停用恢复冲突目录）。
    let disabled = Settings {
        sources: AgentSources {
            workbuddy: None,
            workbuddy_ai: None,
            claude: Some(SourceConfig {
                enabled: false,
                dir: None,
                ..Default::default()
            }),
            codex: None,
        },
        ..Default::default()
    };
    let dirs = effective_source_dirs_from_settings(&disabled).unwrap();
    assert_eq!(
        dirs,
        vec![
            (AgentKind::Codex, codex_default.clone()),
            (AgentKind::Codex, codex_archived.clone()),
            // WB04：两个 WorkBuddy 未配置（默认启用），与 Codex 一起在场。
            (AgentKind::WorkBuddy, wb_default.clone()),
            (AgentKind::WorkBuddyAi, wba_default.clone()),
        ]
    );

    // Claude 显式撞 Codex 默认根（Codex 缺省）→ 冲突必须被识别。
    let hit_default = Settings {
        sources: AgentSources {
            workbuddy: None,
            workbuddy_ai: None,
            claude: Some(SourceConfig {
                enabled: true,
                dir: Some(codex_default.to_string_lossy().into_owned()),
                ..Default::default()
            }),
            codex: None,
        },
        ..Default::default()
    };
    let dirs = effective_source_dirs_from_settings(&hit_default).unwrap();
    // WB04 口径：五根在场（缺省的 Codex 与 WorkBuddy 仍在生效，参与校验）。
    assert_eq!(dirs.len(), 5, "缺省的 Codex 仍在生效，必须在场参与校验");
    let err = tokenscope::settings::validate_dir_conflict(&dirs[0].1, &dirs[1].1).unwrap_err();
    assert!(err.contains("来源目录冲突"), "{err}");

    // 父/子目录（嵌套）同样冲突：默认根的上层目录不得被另一来源占用。
    let parent = codex_default.parent().unwrap();
    let nested = Settings {
        sources: AgentSources {
            workbuddy: None,
            workbuddy_ai: None,
            claude: Some(SourceConfig {
                enabled: true,
                dir: Some(parent.display().to_string()),
                ..Default::default()
            }),
            codex: None,
        },
        ..Default::default()
    };
    let dirs = effective_source_dirs_from_settings(&nested).unwrap();
    assert!(
        tokenscope::settings::validate_dir_conflict(&dirs[0].1, &dirs[1].1).is_err(),
        "默认根的父目录必须识别为嵌套冲突"
    );
    assert!(
        tokenscope::settings::validate_dir_conflict(&dirs[0].1, &dirs[2].1).is_err(),
        "归档根同样参与嵌套冲突校验"
    );
}

/// Task 2（审阅）：mock source——两个 agent 对同一文件都产出事件，
/// 修复后只保留一份（不依赖某 adapter 恰好解析不了另一种格式）。
mod mock_pair {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    use tokenscope::model::{AgentKind, UsageEvent};
    use tokenscope::source::{CollectStats, FileParse, Source};

    pub struct MockAgent {
        root: PathBuf,
        file: PathBuf,
        pub agent: AgentKind,
        pub parses: std::sync::Arc<AtomicUsize>,
    }

    impl MockAgent {
        pub fn new(root: &Path, file: &Path, agent: AgentKind) -> Self {
            Self {
                root: root.to_path_buf(),
                file: file.to_path_buf(),
                agent,
                parses: std::sync::Arc::new(AtomicUsize::new(0)),
            }
        }
    }

    impl Source for MockAgent {
        fn agent(&self) -> AgentKind {
            self.agent
        }

        fn root(&self) -> &Path {
            &self.root
        }

        fn discover_with_errors(&self) -> (Vec<PathBuf>, Vec<String>) {
            (vec![self.file.clone()], Vec::new())
        }

        fn parse_file(&self, _path: &Path) -> FileParse {
            self.parses.fetch_add(1, Ordering::Relaxed);
            let stats = CollectStats {
                lines_seen: 1,
                events: 1,
                ..CollectStats::default()
            };
            FileParse {
                stats,
                events: vec![UsageEvent {
                    cache_write_5m: 0,
                    cache_write_1h: 0,
                    cache_write_split_known: false,
                    ts: "2026-08-01T10:00:00Z".parse().unwrap(),
                    agent: self.agent,
                    model: "m".into(),
                    session_id: "s".into(),
                    project: "p".into(),
                    session_initial_cwd: None,
                    event_cwd: None,
                    record_id: String::new(),
                    line: 0,
                    source_path: String::new(),
                    input_tokens: 100,
                    output_tokens: 10,
                    cache_write_tokens: 0,
                    cache_read_tokens: 0,
                }],
            }
        }
    }
}

/// PR3 §8.3：两个 adapter 认领同一规范化文件 → 该文件**隔离**（两侧都不
/// 解析）并诊断；其余来源照常。硬拒绝移到保存事务（S04）。
#[test]
fn ambiguous_file_claim_is_isolated_before_parsing() {
    let dir = tmp_dir("both-produce");
    let shared = shared_dir(&dir);
    let file = shared.join("s.jsonl");
    let claude =
        mock_pair::MockAgent::new(&shared, &file, tokenscope::model::AgentKind::ClaudeCode);
    let codex = mock_pair::MockAgent::new(&shared, &file, tokenscope::model::AgentKind::Codex);
    let claude_parses = claude.parses.clone();
    let codex_parses = codex.parses.clone();
    let sources: Vec<Box<dyn tokenscope::source::Source>> = vec![Box::new(claude), Box::new(codex)];
    let collected = tokenscope::report::collect_all_with_sources_for_test(
        sources,
        &SummaryOptions {
            workbuddy_enabled: Some(false),
            workbuddy_ai_enabled: Some(false),

            by: GroupBy::Day,
            cache_dir: Some(dir.join("cache")),
            pricing_index: Some(dir.join("idx.json")),
            pricing_path: Some(dir.join("no-pricing.toml")),
            openrouter_path: Some(PathBuf::from("Z:/no-such/or.json")),
            modelsdev_path: Some(PathBuf::from("Z:/no-such/md.json")),
            tz: Some("Asia/Shanghai".to_string()),
            ..Default::default()
        },
    );
    let collected = collected.expect("跨 adapter 同文件隔离后采集成功");
    assert!(collected.events.is_empty(), "冲突文件两侧都不产出事件");
    assert!(
        collected
            .warnings
            .iter()
            .any(|w| w.contains("文件归属冲突")),
        "隔离必须诊断可见: {:?}",
        collected.warnings
    );
    assert_eq!(
        claude_parses.load(Ordering::Relaxed) + codex_parses.load(Ordering::Relaxed),
        0,
        "隔离发生在解析之前"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// SF09：同 adapter 内的重复发现路径不构成跨 adapter 归属冲突；
/// 完全相同的事件经全局去重后不重复计数。
#[test]
fn same_agent_duplicate_discovery_keeps_single() {
    let dir = tmp_dir("same-agent-dup");
    let shared = shared_dir(&dir);
    let file = shared.join("s.jsonl");
    let claude =
        mock_pair::MockAgent::new(&shared, &file, tokenscope::model::AgentKind::ClaudeCode);
    let claude2 =
        mock_pair::MockAgent::new(&shared, &file, tokenscope::model::AgentKind::ClaudeCode);
    let sources: Vec<Box<dyn tokenscope::source::Source>> =
        vec![Box::new(claude), Box::new(claude2)];
    let collected = tokenscope::report::collect_all_with_sources_for_test(
        sources,
        &SummaryOptions {
            workbuddy_enabled: Some(false),
            workbuddy_ai_enabled: Some(false),

            by: GroupBy::Day,
            cache_dir: Some(dir.join("cache")),
            pricing_index: Some(dir.join("idx.json")),
            pricing_path: Some(dir.join("no-pricing.toml")),
            openrouter_path: Some(PathBuf::from("Z:/no-such/or.json")),
            modelsdev_path: Some(PathBuf::from("Z:/no-such/md.json")),
            tz: Some("Asia/Shanghai".to_string()),
            ..Default::default()
        },
    )
    .expect("同 agent 重复发现不构成归属冲突");
    assert_eq!(
        collected.events.len(),
        1,
        "同 (ts,rid,桶) 的重复事件经全局去重不重复计数"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// PR3：跨应用重叠 → 隔离成功（其余根照常采集、事实落库）；
/// 分离目录的冷/热缓存统计不受影响。
#[test]
fn test_source_overlap_isolated_writes_cache_and_split_dirs_unaffected() {
    let dir = tmp_dir("cache-order");
    let shared = shared_dir(&dir);
    // 重叠配置：隔离成功，claude 的文件照常入库。
    let o = opts(&dir, &shared, Some(shared.clone()));
    let r = summary(&o).unwrap();
    assert_eq!(r.totals.requests, 1);
    let info = cache_stats(o.cache_dir.clone()).unwrap();
    assert_eq!(info.files, 1, "隔离采集照常写缓存（仅未隔离根）");

    // 分离目录（codex 指向独立目录）：冷/热两次采集缓存行稳定。
    let codex_own = dir.join("codex-own");
    std::fs::create_dir_all(&codex_own).unwrap();
    let meta = r#"{"timestamp":"2026-08-01T10:00:00.000Z","type":"session_meta","payload":{"id":"s1","session_id":"x1","cwd":"C:/w/x"}}"#;
    let turn = r#"{"timestamp":"2026-08-01T10:01:00.000Z","type":"turn_context","payload":{"turn_id":"t1","model":"gpt-5.6-sol","cwd":"C:/w/x"}}"#;
    let usage = r#"{"timestamp":"2026-08-01T10:02:00.000Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":10,"output_tokens":5,"cached_input_tokens":0,"cache_write_input_tokens":0,"reasoning_output_tokens":0,"total_tokens":15}}}}"#;
    std::fs::write(
        codex_own.join("rollout-c.jsonl"),
        format!("{meta}\n{turn}\n{usage}\n"),
    )
    .unwrap();
    let o_split = opts(&dir, &shared, Some(codex_own));
    let r1 = summary(&o_split).unwrap();
    let first = cache_stats(o_split.cache_dir.clone()).unwrap();
    let r2 = summary(&o_split).unwrap();
    let second = cache_stats(o_split.cache_dir.clone()).unwrap();
    assert_eq!(first.files, second.files, "热采集缓存行稳定");
    assert_eq!(r1.totals.requests, r2.totals.requests, "冷/热统计一致");
    assert_eq!(r1.totals.requests, 2, "claude 1 + codex 1");
    std::fs::remove_dir_all(&dir).ok();
}
