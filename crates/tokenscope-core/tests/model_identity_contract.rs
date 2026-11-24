//! MP03/MP04：可信模型展示名 + 模型身份分组与下钻。
//!
//! 计划：docs/plans/active/2026-10-10-model-pricing-name-equivalence.md §2.12/§3。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use tokenscope::aggregate::GroupBy;
use tokenscope::model::TokenCounts;
use tokenscope::model_identity::ModelIdentity;
use tokenscope::modelsdev::{load_snapshot, sync_with_body_for_tests};
use tokenscope::pricing::Pricing;
use tokenscope::report::{EventFilter, SummaryOptions, summary};

static SEQ: AtomicUsize = AtomicUsize::new(0);

fn tmp(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let d = std::env::temp_dir().join(format!(
        "tokenscope-mp03-{tag}-{}-{n}-{}",
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

fn counts(input: u64, output: u64, cw: u64, cr: u64) -> TokenCounts {
    TokenCounts {
        input,
        output,
        cache_write: cw,
        cache_read: cr,
        cache_write_5m: 0,
        cache_write_1h: 0,
        cache_write_split_known: false,
    }
}

fn at() -> jiff::Timestamp {
    "2026-10-07T05:00:00Z".parse().unwrap()
}

fn sync_to(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    sync_with_body_for_tests(&path, body).unwrap();
    path
}

// ---- MP04：模型身份分组、下钻与守恒 ----

/// Claude 合成日志目录：每行一条 assistant 用量事件（同一会话、同一 cwd）。
fn claude_dir(dir: &Path, models: &[&str], input: u64) -> PathBuf {
    let project = dir.join("claude").join("alpha");
    std::fs::create_dir_all(&project).unwrap();
    let mut body = String::new();
    for (i, model) in models.iter().enumerate() {
        let ts = format!("2026-10-07T05:{:02}:{:02}.000Z", i / 60, i % 60);
        body.push_str(&format!(
            r#"{{"type":"assistant","timestamp":"{ts}","sessionId":"s1","cwd":"C:/work/alpha","message":{{"id":"m-{i}","model":"{model}","usage":{{"input_tokens":{input},"output_tokens":1,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}}}}"#
        ));
        body.push('\n');
    }
    std::fs::write(project.join("s.jsonl"), body).unwrap();
    dir.join("claude")
}

fn no_codex(dir: &Path) -> PathBuf {
    dir.join("no-codex")
}

/// 指定时间与模型的 Claude 合成事件（逐请求计费场景用）。
fn claude_events(dir: &Path, rows: &[(&str, &str, u64)]) -> PathBuf {
    let project = dir.join("claude").join("alpha");
    std::fs::create_dir_all(&project).unwrap();
    let mut body = String::new();
    for (i, (ts, model, input)) in rows.iter().enumerate() {
        body.push_str(&format!(
            r#"{{"type":"assistant","timestamp":"{ts}","sessionId":"s1","cwd":"C:/work/alpha","message":{{"id":"m-{i}","model":"{model}","usage":{{"input_tokens":{input},"output_tokens":0,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}}}}"#
        ));
        body.push('\n');
    }
    std::fs::write(project.join("s.jsonl"), body).unwrap();
    dir.join("claude")
}

fn opts(dir: &Path, claude: &Path, modelsdev: Option<&Path>, by: GroupBy) -> SummaryOptions {
    SummaryOptions {
        workbuddy_enabled: Some(false),
        workbuddy_ai_enabled: Some(false),

        by,
        claude_dir: Some(claude.to_path_buf()),
        codex_dir: Some(no_codex(dir)),
        cache_dir: Some(dir.join("cache")),
        pricing_index: Some(dir.join("idx.json")),
        pricing_path: Some(dir.join("pricing.toml")),
        openrouter_path: Some(dir.join("or-missing.json")),
        modelsdev_path: Some(
            modelsdev
                .map(Path::to_path_buf)
                .unwrap_or_else(|| dir.join("md-missing.json")),
        ),
        tz: Some("Asia/Shanghai".to_string()),
        ..Default::default()
    }
}

fn list_all(o: &SummaryOptions, filter: &EventFilter) -> tokenscope::report::EventList {
    let snap = tokenscope::query::begin_query(o).unwrap();
    tokenscope::query::query_events(&snap.query_id, filter).unwrap()
}

#[test]
fn model_identity_groups_aliases_across_agents() {
    let dir = tmp("groups");
    let claude = claude_dir(
        &dir,
        &["claude-opus-5-5", "claude-opus-5.5", "CLAUDE_OPUS_5_5"],
        10,
    );
    // 可信 models.dev 名（只挂在其中一个等价 ID 上）。
    let md = sync_to(
        &dir,
        "pricing-modelsdev.json",
        r#"{"acme": {"models": {"claude-opus-5.5": {"name": "Claude Opus 5.5", "cost": {"input": 3, "output": 15}}}}}"#,
    );
    let r = summary(&opts(&dir, &claude, Some(&md), GroupBy::Model)).unwrap();
    assert_eq!(
        r.groups.len(),
        2,
        "三种拼写合并为一行 + 合计：{:?}",
        r.groups
    );
    let g = &r.groups[0];
    // 三种裸拼写的结构等价成立，但价格目录候选唯一不能证明属于 acme。
    // 没有 canonical_model_id 关系时，可信显示名也不会改写分组身份。
    assert_eq!(
        g.key, "claudeopus55",
        "唯一价格候选与展示名都不是跨渠道统计别名的证据"
    );
    assert_eq!(
        g.label.as_deref(),
        Some("Claude Opus 5.5"),
        "优先使用可信 models.dev 模型名"
    );
    assert_eq!(g.requests, 3, "三种拼写都计入同一模型");
    assert_eq!(g.tokens.input, 30);

    // 无可信名称时退回原始代表写法（字典序最小），不展示压缩键。
    let dir2 = tmp("groups-no-name");
    let claude2 = claude_dir(&dir2, &["claude-opus-5.5", "claude-opus-5-5"], 10);
    let r2 = summary(&opts(&dir2, &claude2, None, GroupBy::Model)).unwrap();
    assert_eq!(r2.groups.len(), 2);
    assert_eq!(r2.groups[0].key, "claudeopus55");
    assert_eq!(r2.groups[0].label.as_deref(), Some("claude-opus-5-5"));
}

#[test]
fn model_display_name_requires_trusted_source() {
    let dir = tmp("trusted-source");
    // models.dev 提供可信模型名；OpenRouter 的同等价 ID 只有渠道别名。
    let md = sync_to(
        &dir,
        "pricing-modelsdev.json",
        r#"{"acme": {"models": {"opus-5.5": {"name": "Trusted Model Name", "cost": {"input": 3, "output": 15}}}}}"#,
    );
    let orp = dir.join("openrouter.json");
    std::fs::write(
        &orp,
        r#"{"v":2,"synced_at":"t","entries":[{"id":"alias/opus5.5","name":"Channel Alias","prompt":0.000004,"completion":0.00002}]}"#,
    )
    .unwrap();
    let (p, _w) = Pricing::load(None, Some(&md), Some(&orp));
    assert_eq!(
        p.display_name_for("opus55").as_deref(),
        Some("Trusted Model Name"),
        "展示名只认可信来源（models.dev）的模型级 name"
    );

    // 只有 OpenRouter 时没有可信名 → None（聚合退回原始代表写法）。
    let dir2 = tmp("trusted-source-or-only");
    let orp2 = dir2.join("openrouter.json");
    std::fs::write(
        &orp2,
        r#"{"v":2,"synced_at":"t","entries":[{"id":"alias/opus5.5","name":"Channel Alias","prompt":0.000004,"completion":0.00002}]}"#,
    )
    .unwrap();
    let (p2, _w2) = Pricing::load(None, None, Some(&orp2));
    assert_eq!(
        p2.display_name_for("opus55"),
        None,
        "渠道别名不得成为模型展示名"
    );
}

/// v0.9.8（DN01）：302ai 的 name 是末段 ID 原样，字典序排在官方/正式条目前，
/// 旧逻辑会盖掉 `Claude Opus 4.7` 等正式名。名称质量分级后应始终选正式名。
#[test]
fn model_display_name_prefers_formal_name_over_id_copy() {
    let dir = tmp("formal-over-id-copy");
    // 302ai 关键在前，其 name 是末段原样；abacus/anthropic/openai 给正式名。
    let body = r#"{
        "302ai": {"models": {
            "claude-opus-4-7": {"name": "claude-opus-4-7", "cost": {"input": 3, "output": 15}},
            "claude-sonnet-4-6": {"name": "claude-sonnet-4-6", "cost": {"input": 3, "output": 15}},
            "gpt-5.4": {"name": "gpt-5.4", "cost": {"input": 3, "output": 15}}
        }},
        "abacus": {"models": {
            "claude-opus-4-7": {"name": "Claude Opus 4.7", "cost": {"input": 3, "output": 15}},
            "claude-sonnet-4-6": {"name": "Claude Sonnet 4.6", "cost": {"input": 3, "output": 15}},
            "gpt-5.4": {"name": "GPT-5.4", "cost": {"input": 3, "output": 15}}
        }},
        "anthropic": {"models": {
            "claude-opus-4-7": {"name": "Claude Opus 4.7", "cost": {"input": 3, "output": 15}},
            "claude-sonnet-4-6": {"name": "Claude Sonnet 4.6", "cost": {"input": 3, "output": 15}}
        }},
        "openai": {"models": {
            "gpt-5.4": {"name": "GPT-5.4", "cost": {"input": 3, "output": 15}}
        }}
    }"#;
    let snap = sync_to(&dir, "md.json", body);
    let (p, _w) = Pricing::load(None, Some(&snap), None);
    assert_eq!(
        p.display_name_for("claude-opus-4-7").as_deref(),
        Some("Claude Opus 4.7"),
        "302ai 的 ID 抄写不得盖掉正式名"
    );
    assert_eq!(
        p.display_name_for("claude-sonnet-4-6").as_deref(),
        Some("Claude Sonnet 4.6")
    );
    assert_eq!(
        p.display_name_for("gpt-5.4").as_deref(),
        Some("GPT-5.4"),
        "短名与标点保留"
    );
    // 目录乱序不影响结果。
    let dir2 = tmp("formal-order-independent");
    let body2 = r#"{
        "openai": {"models": {
            "gpt-5.4": {"name": "GPT-5.4", "cost": {"input": 3, "output": 15}}
        }},
        "anthropic": {"models": {
            "claude-opus-4-7": {"name": "Claude Opus 4.7", "cost": {"input": 3, "output": 15}}
        }},
        "abacus": {"models": {
            "claude-opus-4-7": {"name": "Claude Opus 4.7", "cost": {"input": 3, "output": 15}},
            "gpt-5.4": {"name": "GPT-5.4", "cost": {"input": 3, "output": 15}}
        }},
        "302ai": {"models": {
            "claude-opus-4-7": {"name": "claude-opus-4-7", "cost": {"input": 3, "output": 15}},
            "gpt-5.4": {"name": "gpt-5.4", "cost": {"input": 3, "output": 15}}
        }}
    }"#;
    let snap2 = sync_to(&dir2, "md.json", body2);
    let (p2, _w2) = Pricing::load(None, Some(&snap2), None);
    assert_eq!(
        p2.display_name_for("claude-opus-4-7").as_deref(),
        Some("Claude Opus 4.7")
    );
    assert_eq!(p2.display_name_for("gpt-5.4").as_deref(), Some("GPT-5.4"));
}

/// v0.9.8 补修二（DN10）：Preview 与日期版本是独立模型版本，不得借用基础版
/// 的展示名；目录无该完整 ID 条目时保留原始 ID。
#[test]
fn model_display_name_preview_and_date_versions_do_not_borrow_base() {
    let dir = tmp("preview-date");
    let snap = sync_to(
        &dir,
        "md.json",
        r#"{
            "anthropic": {"models": {
                "claude-sonnet-4-6": {"name": "Claude Sonnet 4.6", "cost": {"input": 3, "output": 15}},
                "claude-sonnet-4-6-preview": {"name": "Claude Sonnet 4.6 Preview", "cost": {"input": 3, "output": 15}}
            }}
        }"#,
    );
    let (p, _w) = Pricing::load(None, Some(&snap), None);
    assert_eq!(
        p.display_name_for("claude-sonnet-4-6-preview").as_deref(),
        Some("Claude Sonnet 4.6 Preview"),
        "Preview 使用自身名称，不借基础版"
    );
    assert_eq!(
        p.display_name_for("claude-sonnet-4-6-20260101"),
        None,
        "日期版本无目录条目时保留原始 ID"
    );
    assert_eq!(
        p.display_name_for("claude-sonnet-4-6").as_deref(),
        Some("Claude Sonnet 4.6"),
        "基础版仍是正式名"
    );
}

/// v0.9.8 补修二（DN07）：aimlapi 把同一模型写成词序变体「Claude 4.6 Sonnet」。
/// 旧规则用名称核心形态的包含关系判身份，与官方「Claude Sonnet 4.6」互不包含
/// 而误判冲突；身份应由完整 ID / 已确认基础模型决定，名称只参与质量排序。
#[test]
fn model_display_name_word_order_variant_is_not_conflict() {
    let dir = tmp("sonnet-word-order");
    let snap = sync_to(
        &dir,
        "md.json",
        r#"{
            "302ai": {"models": {
                "claude-sonnet-4-6": {"name": "claude-sonnet-4-6", "cost": {"input": 3, "output": 15}}
            }},
            "aimlapi": {"models": {
                "claude-sonnet-4-6": {"name": "Claude 4.6 Sonnet", "cost": {"input": 3, "output": 15}}
            }},
            "anthropic": {"models": {
                "claude-sonnet-4-6": {"name": "Claude Sonnet 4.6", "cost": {"input": 3, "output": 15}}
            }}
        }"#,
    );
    let (p, _w) = Pricing::load(None, Some(&snap), None);
    assert_eq!(
        p.display_name_for("claude-sonnet-4-6").as_deref(),
        Some("Claude Sonnet 4.6"),
        "词序变体不是身份冲突，官方正式名优先"
    );
    assert_eq!(
        p.display_name_for("anthropic/claude-sonnet-4-6").as_deref(),
        Some("Claude Sonnet 4.6")
    );
    // 目录乱序不影响结果。
    let dir2 = tmp("sonnet-word-order-reversed");
    let snap2 = sync_to(
        &dir2,
        "md.json",
        r#"{
            "anthropic": {"models": {
                "claude-sonnet-4-6": {"name": "Claude Sonnet 4.6", "cost": {"input": 3, "output": 15}}
            }},
            "aimlapi": {"models": {
                "claude-sonnet-4-6": {"name": "Claude 4.6 Sonnet", "cost": {"input": 3, "output": 15}}
            }},
            "302ai": {"models": {
                "claude-sonnet-4-6": {"name": "claude-sonnet-4-6", "cost": {"input": 3, "output": 15}}
            }}
        }"#,
    );
    let (p2, _w2) = Pricing::load(None, Some(&snap2), None);
    assert_eq!(
        p2.display_name_for("claude-sonnet-4-6").as_deref(),
        Some("Claude Sonnet 4.6")
    );
}

/// v0.9.8 补修二（DN08）：同末段但不同命名空间是不同模型；author-c 没有
/// 名称时不得借用 author-a 的名称，裸 demo-1 也无法确认身份。
#[test]
fn model_display_name_unnamed_namespace_does_not_borrow_sibling() {
    let dir = tmp("unnamed-namespace");
    let snap = sync_to(
        &dir,
        "md.json",
        r#"{
            "author-a": {"models": {
                "demo-1": {"name": "Demo 1", "cost": {"input": 3, "output": 15}}
            }},
            "author-b": {"models": {
                "demo-1": {"name": "Demo 1 Pro", "cost": {"input": 3, "output": 15}}
            }},
            "author-c": {"models": {
                "demo-1": {"cost": {"input": 3, "output": 15}}
            }}
        }"#,
    );
    let (p, _w) = Pricing::load(None, Some(&snap), None);
    assert_eq!(
        p.display_name_for("author-c/demo-1"),
        None,
        "带命名空间且自身无合格名称时保留自身原始 ID，不借同末段渠道"
    );
    assert_eq!(
        p.display_name_for("demo-1"),
        None,
        "裸 ID 无确认身份且同末段多身份冲突，保留原文"
    );
    // 有名称的同末段仍按自身身份解析。
    assert_eq!(
        p.display_name_for("author-a/demo-1").as_deref(),
        Some("Demo 1")
    );
    assert_eq!(
        p.display_name_for("author-b/demo-1").as_deref(),
        Some("Demo 1 Pro")
    );
}

/// v0.9.8 补修二（DN09）：官方条目 name 只是 ID 抄写时，应继续在同一模型
/// 版本的**已确认**渠道里找正式名，而不是直接返回 ID。补修三起，渠道须在目录里
/// 明确声明 `canonical_model_id` 指向该基础模型；仅同末段不构成身份依据。
#[test]
fn model_display_name_id_copy_official_falls_back_to_channel_name() {
    let dir = tmp("official-id-copy");
    let snap = sync_to(
        &dir,
        "md.json",
        r#"{
            "anthropic": {"models": {
                "claude-opus-4-7": {"name": "claude-opus-4-7", "cost": {"input": 3, "output": 15}}
            }},
            "abacus": {"models": {
                "claude-opus-4-7": {
                    "name": "Claude Opus 4.7",
                    "canonical_model_id": "anthropic/claude-opus-4-7",
                    "cost": {"input": 3, "output": 15}
                }
            }}
        }"#,
    );
    let (p, _w) = Pricing::load(None, Some(&snap), None);
    assert_eq!(
        p.display_name_for("anthropic/claude-opus-4-7").as_deref(),
        Some("Claude Opus 4.7"),
        "官方只有 ID 抄写时回落已确认渠道的正式名"
    );
    assert_eq!(
        p.display_name_for("claude-opus-4-7").as_deref(),
        Some("Claude Opus 4.7")
    );
}

/// v0.9.8 补修（DN04）：同为正式名时，302ai 的「Claude 4.7 Opus」字典序靠前，
/// 旧逻辑选中它；应先按确认的基础模型官方完整 ID 选「Claude Opus 4.7」。
#[test]
fn model_display_name_prefers_official_full_id_over_channel_rename() {
    let dir = tmp("official-over-channel-rename");
    let snap = sync_to(
        &dir,
        "md.json",
        r#"{
            "302ai": {"models": {
                "claude-opus-4-7": {"name": "Claude 4.7 Opus", "cost": {"input": 3, "output": 15}}
            }},
            "anthropic": {"models": {
                "claude-opus-4-7": {"name": "Claude Opus 4.7", "cost": {"input": 3, "output": 15}}
            }}
        }"#,
    );
    let (p, _w) = Pricing::load(None, Some(&snap), None);
    // 裸 ID 与确认后的基础键都必须落到官方条目的正式名。
    assert_eq!(
        p.display_name_for("claude-opus-4-7").as_deref(),
        Some("Claude Opus 4.7"),
        "确认到基础模型后优先官方完整 ID 的正式名"
    );
    assert_eq!(
        p.display_name_for("anthropic/claude-opus-4-7").as_deref(),
        Some("Claude Opus 4.7"),
        "带命名空间时按自身完整身份解析"
    );
    assert_eq!(
        p.display_name_for(p.model_stats_key("claude-opus-4-7").as_str())
            .as_deref(),
        Some("Claude Opus 4.7")
    );
}

/// v0.9.8 补修（DN06）：新增排在官方前的转发渠道、目录乱序、官方条目缺名时
/// 的兜底与裸 ID / 确认别名 / 无目录的结果。
#[test]
fn model_display_name_official_priority_edge_cases() {
    // 302ai（正式名）与 aaa-relay（字典序更靠前）都排在官方 anthropic 前。
    let dir = tmp("official-edges");
    let snap = sync_to(
        &dir,
        "md.json",
        r#"{
            "aaa-relay": {"models": {
                "claude-opus-4-7": {"name": "Relay Opus 4.7", "cost": {"input": 3, "output": 15}}
            }},
            "302ai": {"models": {
                "claude-opus-4-7": {"name": "Claude 4.7 Opus", "cost": {"input": 3, "output": 15}}
            }},
            "anthropic": {"models": {
                "claude-opus-4-7": {"name": "Claude Opus 4.7", "cost": {"input": 3, "output": 15}}
            }}
        }"#,
    );
    let (p, _) = Pricing::load(None, Some(&snap), None);
    assert_eq!(
        p.display_name_for("claude-opus-4-7").as_deref(),
        Some("Claude Opus 4.7"),
        "转发渠道乱序与字典序靠前都不改变官方优先结果"
    );
    assert_eq!(
        p.display_name_for("anthropic/claude-opus-4-7").as_deref(),
        Some("Claude Opus 4.7")
    );

    // 官方条目缺名 → 用**已确认**渠道（目录明确指向基础模型）的正式名兜底。
    let dir2 = tmp("official-missing-name");
    let snap2 = sync_to(
        &dir2,
        "md.json",
        r#"{
            "302ai": {"models": {
                "claude-opus-4-7": {
                    "name": "Claude 4.7 Opus",
                    "canonical_model_id": "anthropic/claude-opus-4-7",
                    "cost": {"input": 3, "output": 15}
                }
            }},
            "anthropic": {"models": {
                "claude-opus-4-7": {"cost": {"input": 3, "output": 15}}
            }}
        }"#,
    );
    let (p2, _) = Pricing::load(None, Some(&snap2), None);
    assert_eq!(
        p2.display_name_for("claude-opus-4-7").as_deref(),
        Some("Claude 4.7 Opus"),
        "官方条目无可用正式名时用已确认渠道的正式名兜底"
    );

    // 无目录 → 裸 ID、确认别名都返回 None（保留原始 ID）。
    let (p3, _) = Pricing::load(None, None, None);
    assert_eq!(p3.display_name_for("claude-opus-4-7"), None);
    assert_eq!(p3.display_name_for("gpt-6-astra-cc-format"), None);
}

/// v0.9.8 补修（DN05）：同末段不同命名空间各有正式名，查 B 不得借 A 的名称；
/// 裸 ID 因无法确认身份且正式名冲突，保留原始 ID（返回 None）。
#[test]
fn model_display_name_uses_own_namespace_and_keeps_ambiguous_bare_id() {
    let dir = tmp("namespace-identity");
    let snap = sync_to(
        &dir,
        "md.json",
        r#"{
            "author-a": {"models": {
                "demo-1": {"name": "Demo A One", "cost": {"input": 3, "output": 15}}
            }},
            "author-b": {"models": {
                "demo-1": {"name": "Demo B Two", "cost": {"input": 3, "output": 15}}
            }}
        }"#,
    );
    let (p, _w) = Pricing::load(None, Some(&snap), None);
    assert_eq!(
        p.display_name_for("author-b/demo-1").as_deref(),
        Some("Demo B Two"),
        "带命名空间应使用自身完整身份，不借 author-a 的名称"
    );
    assert_eq!(
        p.display_name_for("author-a/demo-1").as_deref(),
        Some("Demo A One")
    );
    assert_eq!(
        p.display_name_for("demo-1"),
        None,
        "裸 ID 的同末段正式名冲突且无可确认身份时保留原始 ID"
    );
}

/// DN02：没有正式名时，ID 抄写式 name 仍是合法兜底（如 `o3` 的目录名就是 `o3`）。
#[test]
fn model_display_name_id_copy_is_last_resort() {
    let dir = tmp("id-copy-fallback");
    let snap = sync_to(
        &dir,
        "md.json",
        r#"{"302ai": {"models": {"o3": {"name": "o3", "cost": {"input": 3, "output": 15}}}}}"#,
    );
    let (p, _w) = Pricing::load(None, Some(&snap), None);
    assert_eq!(p.display_name_for("o3").as_deref(), Some("o3"));
}

/// DN03：只有 ID 抄写、没有任何正式名时也不凭空造名；无目录条目返回 None。
#[test]
fn model_display_name_untrusted_id_returns_none() {
    let (p, _w) = Pricing::load(None, None, None);
    assert_eq!(
        p.display_name_for("gpt-6-astra-cc-format"),
        None,
        "无目录记录保留原始 ID"
    );
    assert_eq!(p.display_name_for("grok-4.6-build"), None);
    assert_eq!(p.display_name_for("glm-5.2-x"), None);
}

/// DN11（v0.9.8 补修三）：同末段只是检索键，**不是身份证据**。查询模型的基础
/// 身份确认后，每条渠道候选仍须单独核对——自身完整 ID 等于目标，或目录关系
/// 明确指向同一基础模型，才可参与选名；缺少关系、明确指向其它模型或关系冲突
/// 的渠道一律排除。合成目录复现：官方 name 只是原始 ID，`known-relay` 明确指向
/// Anthropic 基础模型，`aaa` 明确指向另一个模型且字典序最靠前。
#[test]
fn model_display_name_requires_per_candidate_identity() {
    let dir = tmp("candidate-identity");
    let snap = sync_to(
        &dir,
        "md.json",
        r#"{
            "aaa": {"models": {
                "claude-opus-4-7": {
                    "name": "Independent Model",
                    "canonical_model_id": "independent/claude-opus-4-7",
                    "cost": {"input": 3, "output": 15}
                }
            }},
            "anthropic": {"models": {
                "claude-opus-4-7": {"name": "claude-opus-4-7", "cost": {"input": 3, "output": 15}}
            }},
            "known-relay": {"models": {
                "claude-opus-4-7": {
                    "name": "Claude Opus 4.7",
                    "canonical_model_id": "anthropic/claude-opus-4-7",
                    "cost": {"input": 3, "output": 15}
                }
            }}
        }"#,
    );
    let (p, _w) = Pricing::load(None, Some(&snap), None);
    assert_eq!(
        p.display_name_for("claude-opus-4-7").as_deref(),
        Some("Claude Opus 4.7"),
        "明确指向其它模型的同末段渠道不得参与选名"
    );
    assert_eq!(
        p.display_name_for("anthropic/claude-opus-4-7").as_deref(),
        Some("Claude Opus 4.7"),
        "带命名空间的官方查询同样只认已确认身份的渠道"
    );

    // 对照一：再加一条字典序更靠前、但**没有任何身份关系**的渠道——仍选正式名。
    let dir2 = tmp("candidate-identity-unverified");
    let snap2 = sync_to(
        &dir2,
        "md.json",
        r#"{
            "aab": {"models": {
                "claude-opus-4-7": {"name": "Unverified Model", "cost": {"input": 3, "output": 15}}
            }},
            "anthropic": {"models": {
                "claude-opus-4-7": {"name": "claude-opus-4-7", "cost": {"input": 3, "output": 15}}
            }},
            "known-relay": {"models": {
                "claude-opus-4-7": {
                    "name": "Claude Opus 4.7",
                    "canonical_model_id": "anthropic/claude-opus-4-7",
                    "cost": {"input": 3, "output": 15}
                }
            }}
        }"#,
    );
    let (p2, _w2) = Pricing::load(None, Some(&snap2), None);
    assert_eq!(
        p2.display_name_for("claude-opus-4-7").as_deref(),
        Some("Claude Opus 4.7"),
        "无身份依据的渠道即使字典序更靠前也不参与选名"
    );

    // 对照二：移除唯一合格渠道的名称，回退该组原始 ID（官方条目只剩 ID 抄写）。
    let dir3 = tmp("candidate-identity-no-channel-name");
    let snap3 = sync_to(
        &dir3,
        "md.json",
        r#"{
            "aaa": {"models": {
                "claude-opus-4-7": {
                    "name": "Independent Model",
                    "canonical_model_id": "independent/claude-opus-4-7",
                    "cost": {"input": 3, "output": 15}
                }
            }},
            "anthropic": {"models": {
                "claude-opus-4-7": {"name": "claude-opus-4-7", "cost": {"input": 3, "output": 15}}
            }},
            "known-relay": {"models": {
                "claude-opus-4-7": {
                    "canonical_model_id": "anthropic/claude-opus-4-7",
                    "cost": {"input": 3, "output": 15}
                }
            }}
        }"#,
    );
    let (p3, _w3) = Pricing::load(None, Some(&snap3), None);
    assert_eq!(
        p3.display_name_for("claude-opus-4-7").as_deref(),
        Some("claude-opus-4-7"),
        "无合格正式名时回退该组原始 ID，不借其它模型的名称"
    );

    // 对照二补充：连官方 ID 抄写也没有时，解析器返回缺失，界面保留原始 ID。
    let dir4 = tmp("candidate-identity-no-name");
    let snap4 = sync_to(
        &dir4,
        "md.json",
        r#"{
            "aaa": {"models": {
                "claude-opus-4-7": {
                    "name": "Independent Model",
                    "canonical_model_id": "independent/claude-opus-4-7",
                    "cost": {"input": 3, "output": 15}
                }
            }},
            "anthropic": {"models": {
                "claude-opus-4-7": {"cost": {"input": 3, "output": 15}}
            }},
            "known-relay": {"models": {
                "claude-opus-4-7": {
                    "canonical_model_id": "anthropic/claude-opus-4-7",
                    "cost": {"input": 3, "output": 15}
                }
            }}
        }"#,
    );
    let (p4, _w4) = Pricing::load(None, Some(&snap4), None);
    assert_eq!(
        p4.display_name_for("claude-opus-4-7"),
        None,
        "整组没有合格名称时返回缺失，由聚合保留原始 ID"
    );
}

#[test]
fn blank_model_name_is_unavailable() {
    let dir = tmp("blank-name");
    let md = sync_to(
        &dir,
        "pricing-modelsdev.json",
        r#"{"acme": {"models": {"opus-5.5": {"name": "   ", "cost": {"input": 3, "output": 15}}}}}"#,
    );
    let (p, _w) = Pricing::load(None, Some(&md), None);
    assert_eq!(p.display_name_for("opus55"), None, "空白名称视为缺失");

    // 聚合标签必须退回原始代表写法，不能显示空白。
    let claude = claude_dir(&dir, &["claude-opus-5.5"], 10);
    let r = summary(&opts(&dir, &claude, Some(&md), GroupBy::Model)).unwrap();
    assert_eq!(r.groups[0].key, "claudeopus55");
    assert_eq!(
        r.groups[0].label.as_deref(),
        Some("claude-opus-5.5"),
        "空白名称不得成为展示名"
    );
}

#[test]
fn model_identity_group_cost_uses_per_request_tiers_and_schedules() {
    // 场景 1：上下文分段阈值——两条各自落在基础档的请求，若被错误地
    // "先合并 token 再计价" 就会跨过 272K 阈值按高档计费（金额翻倍）。
    let dir = tmp("per-request-tier");
    let snap = dir.join("pricing-modelsdev.json");
    std::fs::write(
        &snap,
        r#"{"v":4,"synced_at":"t","entries":[{"id":"acme/claude-opus-5.5","name":"Opus 5.5",
            "input":4.0,"output":20.0,
            "segments":[{"min_tokens":272001,"input":8.0,"output":40.0}]}]}"#,
    )
    .unwrap();
    let claude = claude_events(
        &dir,
        &[
            ("2026-10-07T05:00:00.000Z", "claude-opus-5-5", 200_000),
            ("2026-10-07T06:00:00.000Z", "claude-opus-5.5", 100_000),
        ],
    );
    let o = opts(&dir, &claude, Some(&snap), GroupBy::Model);
    let r = summary(&o).unwrap();
    let g = &r.groups[0];
    assert_eq!(g.requests, 2);
    let merged_would_cost = 300_000.0 * 8.0 / 1e6;
    assert!(
        (g.cost_usd - 1.2).abs() < 1e-9,
        "逐请求计价应为 200k×4 + 100k×4 = 1.2/1M，实际 {}",
        g.cost_usd
    );
    assert!(
        (g.cost_usd - merged_would_cost).abs() > 1e-9,
        "不得按合并后的高档计费（{merged_would_cost}）"
    );
    let events = list_all(&o, &EventFilter::default());
    let per_request: f64 = events.rows.iter().filter_map(|row| row.cost_usd).sum();
    assert!(
        (per_request - g.cost_usd).abs() < 1e-9,
        "明细逐条之和必须等于分组费用"
    );

    // 场景 2：时间档（峰谷）——两条请求落在不同档，分组费用 = 各档之和。
    let dir2 = tmp("per-request-schedule");
    let ext = dir2.join("pricing.toml");
    std::fs::write(
        &ext,
        r#"
[[model]]
prefix = "claude-opus-5-5"
input = 2.0
[[model.schedule]]
label = "peak"
timezone = "UTC"
[[model.schedule.period]]
start_time = "09:00"
end_time = "18:00"
input = 6.0
"#,
    )
    .unwrap();
    let claude2 = claude_events(
        &dir2,
        &[
            ("2026-10-07T10:00:00.000Z", "claude-opus-5-5", 1_000_000),
            ("2026-10-07T02:00:00.000Z", "claude-opus-5-5", 1_000_000),
        ],
    );
    let o2 = opts(&dir2, &claude2, None, GroupBy::Model);
    let r2 = summary(&o2).unwrap();
    let g2 = &r2.groups[0];
    assert_eq!(g2.requests, 2);
    assert!(
        (g2.cost_usd - 8.0).abs() < 1e-9,
        "峰 6.0 + 谷 2.0 = 8.0/1M，实际 {}",
        g2.cost_usd
    );
    let events2 = list_all(&o2, &EventFilter::default());
    let per_request2: f64 = events2.rows.iter().filter_map(|row| row.cost_usd).sum();
    assert!((per_request2 - g2.cost_usd).abs() < 1e-9);
}

#[test]
fn model_identity_preserves_usage_and_requests() {
    let dir = tmp("conservation");
    let models = ["claude-opus-5-5", "claude-opus-5.5", "claude-opus-5-5"];
    let claude = claude_dir(&dir, &models, 100);
    let r = summary(&opts(&dir, &claude, None, GroupBy::Model)).unwrap();
    let g = &r.groups[0];
    assert_eq!(g.requests, 3);
    assert_eq!(g.tokens.input, 300);
    assert_eq!(g.tokens.output, 3);
    assert_eq!(r.totals.requests, 3);
    assert_eq!(r.totals.tokens, g.tokens, "合并只重排分组，不改总量");
}

#[test]
fn model_identity_keeps_versions_and_variants_separate() {
    let dir = tmp("separate");
    let claude = claude_dir(
        &dir,
        &[
            "claude-opus-5-5",
            "claude-opus-5-5-20260101",
            "claude-opus-5-5:free",
            "claude-opus-5-6",
        ],
        10,
    );
    let r = summary(&opts(&dir, &claude, None, GroupBy::Model)).unwrap();
    let keys: Vec<&str> = r.groups.iter().map(|g| g.key.as_str()).collect();
    assert_eq!(
        keys,
        [
            "claudeopus55",
            "claudeopus5520260101",
            "claudeopus55:free",
            "claudeopus56",
            "合计"
        ],
        "不同版本与变体不得合并（前缀相同也不行）"
    );
    assert!(
        r.groups[..r.groups.len() - 1]
            .iter()
            .all(|g| g.requests == 1),
        "每个身份各一条请求"
    );
}

#[test]
fn model_identity_group_cost_sums_per_request() {
    let dir = tmp("cost");
    // 两条请求：token 不同 → 分组费用必须等于逐请求费用之和（不能先合并再计价）。
    let project = dir.join("claude").join("alpha");
    std::fs::create_dir_all(&project).unwrap();
    let mut body = String::new();
    for (i, (model, input)) in [
        ("claude-opus-5-5", 1_000_000u64),
        ("claude-opus-5.5", 2_000_000u64),
    ]
    .iter()
    .enumerate()
    {
        body.push_str(&format!(
            r#"{{"type":"assistant","timestamp":"2026-10-07T0{}:00:00.000Z","sessionId":"s1","cwd":"C:/work/alpha","message":{{"id":"c-{i}","model":"{model}","usage":{{"input_tokens":{input},"output_tokens":0,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}}}}"#,
            i + 5
        ));
        body.push('\n');
    }
    std::fs::write(project.join("s.jsonl"), body).unwrap();
    let md = sync_to(
        &dir,
        "pricing-modelsdev.json",
        r#"{"acme": {"models": {"claude-opus-5.5": {"name": "Opus 5.5", "cost": {"input": 3, "output": 15}}}}}"#,
    );
    let o = opts(&dir, &dir.join("claude"), Some(&md), GroupBy::Model);
    let r = summary(&o).unwrap();
    let g = &r.groups[0];
    assert_eq!(g.requests, 2);
    assert!(
        (g.cost_usd - 9.0).abs() < 1e-9,
        "1M×3 + 2M×3 = 9：逐请求计价后求和，{}",
        g.cost_usd
    );

    // 明细逐条费用与分组一致（同源）。
    let events = list_all(
        &o,
        &EventFilter {
            model: Some(g.key.clone()),
            ..Default::default()
        },
    );
    let sum: f64 = events.rows.iter().filter_map(|row| row.cost_usd).sum();
    assert!((sum - g.cost_usd).abs() < 1e-9, "{sum} vs {}", g.cost_usd);
    assert_eq!(events.total, 2);
}

#[test]
fn model_identity_drill_pages_include_all_spellings() {
    let dir = tmp("pages");
    // 210 条事件、两种拼写交替 → 下钻必须跨页返回全部等价请求。
    let models: Vec<&str> = (0..210)
        .map(|i| {
            if i % 2 == 0 {
                "claude-opus-5-5"
            } else {
                "claude-opus-5.5"
            }
        })
        .collect();
    let project = dir.join("claude").join("alpha");
    std::fs::create_dir_all(&project).unwrap();
    let mut body = String::new();
    for (i, model) in models.iter().enumerate() {
        let ts = format!(
            "2026-10-07T{:02}:{:02}:{:02}.000Z",
            5 + i / 3600,
            (i / 60) % 60,
            i % 60
        );
        body.push_str(&format!(
            r#"{{"type":"assistant","timestamp":"{ts}","sessionId":"s1","cwd":"C:/work/alpha","message":{{"id":"p-{i}","model":"{model}","usage":{{"input_tokens":10,"output_tokens":1,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}}}}"#
        ));
        body.push('\n');
    }
    std::fs::write(project.join("s.jsonl"), body).unwrap();

    // N08：给等价键挂上有可信名的价格 → 逐请求费用可求和核对；两种拼写
    // 都经等价候选命中同一价格。
    let md = sync_to(
        &dir,
        "pricing-modelsdev.json",
        r#"{"acme": {"models": {"claude-opus-5.5": {"name": "Claude Opus 5.5", "cost": {"input": 3, "output": 15}}}}}"#,
    );
    let o = opts(&dir, &dir.join("claude"), Some(&md), GroupBy::Model);
    let r = summary(&o).unwrap();
    assert_eq!(r.groups[0].requests, 210, "两种拼写全部合并");
    assert!(r.groups[0].cost_usd > 0.0, "模型已计价");

    let key = r.groups[0].key.clone();
    let snap = tokenscope::query::begin_query(&o).unwrap();
    let page1 = tokenscope::query::query_events(
        &snap.query_id,
        &EventFilter {
            model: Some(key.clone()),
            limit: Some(200),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(page1.total, 210);
    assert_eq!(page1.rows.len(), 200);
    let page2 = tokenscope::query::query_events(
        &snap.query_id,
        &EventFilter {
            model: Some(key.clone()),
            limit: Some(200),
            before: Some(page1.rows.last().unwrap().cursor.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(page2.total, 210, "total 是过滤后全量，不随游标截断");
    assert_eq!(page2.rows.len(), 10, "续页补齐剩余 10 条");
    let all: Vec<_> = page1.rows.iter().chain(page2.rows.iter()).collect();
    let ids: std::collections::BTreeSet<String> =
        all.iter().map(|row| row.record_id.clone()).collect();
    assert_eq!(ids.len(), 210, "跨页不重不漏");
    assert!(all.iter().all(|row| row.project == "C:/work/alpha"));
    // N08：明细保留**原始模型名**（两种拼写都在，不压缩成等价键）。
    let raw: std::collections::BTreeSet<&str> = all.iter().map(|row| row.model.as_str()).collect();
    assert_eq!(
        raw,
        std::collections::BTreeSet::from(["claude-opus-5-5", "claude-opus-5.5"]),
        "分页保留原始模型名"
    );
    // N08：逐请求费用求和 == 汇总费用（210 条全部有价、无 unknown 混入）。
    let per_request: f64 = all.iter().filter_map(|row| row.cost_usd).sum();
    assert!(
        (per_request - r.groups[0].cost_usd).abs() < 1e-6,
        "逐请求费用求和 {per_request} vs 汇总 {}",
        r.groups[0].cost_usd
    );
    assert!(all.iter().all(|row| row.cost_usd.is_some()), "无未计价请求");
}

#[test]
fn model_display_name_uses_model_not_provider() {
    let dir = tmp("provider");
    let snap = sync_to(
        &dir,
        "pricing-modelsdev.json",
        r#"{
            "acme": {"name": "Acme Provider", "models": {
                "opus-5.5": {"name": "Opus 5.5", "cost": {"input": 3, "output": 15}},
                "legacy-x": {"cost": {"input": 1, "output": 2}}
            }}
        }"#,
    );
    let s = load_snapshot(&snap).unwrap().unwrap();
    assert_eq!(s.v, 4, "同步产出的快照必须是 v4");
    let by = |id: &str| s.entries.iter().find(|e| e.id == id).unwrap();
    assert_eq!(by("acme/opus-5.5").name.as_deref(), Some("Opus 5.5"));
    assert_eq!(
        by("acme/legacy-x").name,
        None,
        "模型无 name 时不得用 provider 名兜底"
    );
}

#[test]
fn model_display_name_is_not_identity() {
    let dir = tmp("name-identity");
    let snap = sync_to(
        &dir,
        "pricing-modelsdev.json",
        r#"{
            "acme": {"models": {
                "opus-5.5": {"name": "Shared Name", "cost": {"input": 3, "output": 15}},
                "other-model": {"name": "Shared Name", "cost": {"input": 5, "output": 25}}
            }}
        }"#,
    );
    let (p, _warn) = Pricing::load(None, Some(&snap), None);
    // 名称相同不等于身份相同：两个 ID 各自命中自己的 name 与价格。
    assert_eq!(
        p.display_name_for("acme/opus55").as_deref(),
        Some("Shared Name")
    );
    assert_eq!(
        p.display_name_for("acme/other_model").as_deref(),
        Some("Shared Name")
    );
    assert_ne!(
        ModelIdentity::parse("acme/opus55").identity_key(),
        ModelIdentity::parse("acme/other-model").identity_key()
    );
    let a = p
        .estimate("acme/opus55", &counts(1_000_000, 0, 0, 0), at())
        .unwrap();
    let b = p
        .estimate("acme/other-model", &counts(1_000_000, 0, 0, 0), at())
        .unwrap();
    assert!((a.cost - 3.0).abs() < 1e-9, "{}", a.cost);
    assert!((b.cost - 5.0).abs() < 1e-9, "{}", b.cost);
}

#[test]
fn model_display_name_is_order_independent() {
    let dir = tmp("order");
    let body_acme_first = r#"{
        "acme": {"models": {"opus-5.5": {"name": "Acme Name", "cost": {"input": 3, "output": 15}}}},
        "beta": {"models": {"opus55": {"name": "Beta Name", "cost": {"input": 5, "output": 25}}}}
    }"#;
    let body_beta_first = r#"{
        "beta": {"models": {"opus55": {"name": "Beta Name", "cost": {"input": 5, "output": 25}}}},
        "acme": {"models": {"opus-5.5": {"name": "Acme Name", "cost": {"input": 3, "output": 15}}}}
    }"#;
    let s1 = sync_to(&dir, "a.json", body_acme_first);
    let s2 = sync_to(&dir, "b.json", body_beta_first);
    let (p1, _) = Pricing::load(None, Some(&s1), None);
    let (p2, _) = Pricing::load(None, Some(&s2), None);
    assert_eq!(
        p1.display_name_for("opus55"),
        p2.display_name_for("opus55"),
        "同一等价 ID 下的展示名必须与条目顺序无关"
    );
    // v0.9.8 补修：acme / beta 是两个不同命名空间且各有正式名，裸 opus55
    // 无法确认基础身份 → 保留原始 ID，不再按字典序借用其中一方的名称。
    assert_eq!(
        p1.display_name_for("opus55").as_deref(),
        None,
        "无法确认的同末段冲突保留原始 ID"
    );
    // 带命名空间的查询仍按自身完整身份解析到各自正式名。
    assert_eq!(
        p1.display_name_for("acme/opus55").as_deref(),
        Some("Acme Name")
    );
    assert_eq!(
        p1.display_name_for("beta/opus55").as_deref(),
        Some("Beta Name")
    );
}

#[test]
fn model_display_name_requires_full_id_match() {
    let dir = tmp("full-id");
    let snap = sync_to(
        &dir,
        "pricing-modelsdev.json",
        r#"{"acme": {"models": {"opus-5.5": {"name": "Opus 5.5", "cost": {"input": 3, "output": 15}}}}}"#,
    );
    let (p, _) = Pricing::load(None, Some(&snap), None);
    assert_eq!(
        p.display_name_for("Opus_5.5").as_deref(),
        Some("Opus 5.5"),
        "等价 ID 命中"
    );
    assert!(
        p.display_name_for("opus5.5-20261010").is_none(),
        "仅前缀命中的旧版本不得冒充本模型的展示名"
    );
    assert!(
        p.estimate("opus5.5-20261010", &counts(1_000_000, 0, 0, 0), at())
            .is_some(),
        "定价允许边界前缀回退（与展示名解析相互独立）"
    );
}

#[test]
fn model_display_name_legacy_snapshot_falls_back() {
    let dir = tmp("legacy");
    let snap = dir.join("pricing-modelsdev.json");
    // v3 旧快照：name 可能是供应商名称（旧同步把 provider name 兜底进来）。
    std::fs::write(
        &snap,
        r#"{"v":3,"synced_at":"t","entries":[{"id":"acme/opus-5.5","name":"Acme Provider","input":3.0,"output":15.0}]}"#,
    )
    .unwrap();
    let s = load_snapshot(&snap).unwrap().unwrap();
    assert_eq!(s.entries[0].name, None, "来源不明的 name 不得保留");
    let (p, _) = Pricing::load(None, Some(&snap), None);
    assert!(
        p.display_name_for("opus55").is_none(),
        "无可信 name → 聚合退回原始代表写法"
    );
    assert!(
        p.estimate("opus55", &counts(1_000_000, 0, 0, 0), at())
            .is_some(),
        "旧快照仍可离线提供价格"
    );
}

/// N08：同一模型的等价写法**跨应用**合并——Claude 与 Codex 各自的拼写归入
/// 同一等价身份组；下钻跨页返回两个应用的原始模型名（保留原值，不压缩）。
#[test]
fn model_identity_groups_equivalent_spellings_across_apps() {
    let dir = tmp("cross-app");
    // Claude 侧用连字符拼写，Codex 侧用点号拼写 → 同一等价键 claudeopus55。
    let claude = claude_dir(&dir, &["claude-opus-5-5"], 10);
    let codex = dir.join("codex");
    let session_dir = codex.join("sessions").join("2026").join("07").join("17");
    std::fs::create_dir_all(&session_dir).unwrap();
    std::fs::write(
        session_dir.join("rollout-x.jsonl"),
        format!(
            "{}\n{}\n{}\n",
            r#"{"timestamp":"2026-10-07T05:00:00.000Z","type":"session_meta","payload":{"id":"cx","session_id":"cx","cwd":"C:/work/beta"}}"#,
            r#"{"timestamp":"2026-10-07T05:01:00.000Z","type":"turn_context","payload":{"turn_id":"t","model":"claude-opus-5.5","cwd":"C:/work/beta"}}"#,
            r#"{"timestamp":"2026-10-07T05:02:00.000Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":20,"output_tokens":2,"cached_input_tokens":0,"cache_write_input_tokens":0,"reasoning_output_tokens":0,"total_tokens":22}}}}"#
        ),
    )
    .unwrap();

    let o = SummaryOptions {
        workbuddy_enabled: Some(false),
        workbuddy_ai_enabled: Some(false),

        by: GroupBy::Model,
        claude_dir: Some(claude),
        codex_dir: Some(codex),
        cache_dir: Some(dir.join("cache")),
        pricing_index: Some(dir.join("idx.json")),
        pricing_path: Some(dir.join("pricing.toml")),
        openrouter_path: Some(dir.join("or-missing.json")),
        modelsdev_path: Some(dir.join("md-missing.json")),
        tz: Some("Asia/Shanghai".to_string()),
        ..Default::default()
    };
    let r = summary(&o).unwrap();
    assert_eq!(
        r.groups.len(),
        2,
        "两个应用的等价拼写合并为一行 + 合计：{:?}",
        r.groups
    );
    assert_eq!(r.groups[0].key, "claudeopus55");
    assert_eq!(r.groups[0].requests, 2, "跨应用的同模型请求合并计数");

    // 下钻：一页拿全两条，原始模型名各自保留（不压缩成等价键）。
    let key = r.groups[0].key.clone();
    let snap = tokenscope::query::begin_query(&o).unwrap();
    let page = tokenscope::query::query_events(
        &snap.query_id,
        &EventFilter {
            model: Some(key),
            limit: Some(50),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(page.rows.len(), 2);
    let mut agents: Vec<&str> = page.rows.iter().map(|row| row.agent).collect();
    agents.sort();
    assert_eq!(agents, ["claude-code", "codex"], "下钻包含两个应用");
    let mut models: Vec<&str> = page.rows.iter().map(|row| row.model.as_str()).collect();
    models.sort();
    assert_eq!(
        models,
        ["claude-opus-5-5", "claude-opus-5.5"],
        "明细保留原始模型名"
    );
    std::fs::remove_dir_all(&dir).ok();
}
