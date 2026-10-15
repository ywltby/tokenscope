//! 隔离根原生验收的**归属前哨**（阶段 A/B 的项目身份验收）。
//!
//! 依赖 `scripts/prepare-native-acceptance.ps1` 生成的隔离根：脚本写入的合成
//! Claude/Codex 日志覆盖了"同路径跨工具合并"与"越界切换 + 子目录归并"两种
//! 形态。本测试在**非 GUI** 路径上核对同一批数据的归属，作为 GUI 目视验收
//! 之前的自动化前哨（GUI 窗口观感仍需人工确认，见计划 A06/B03）。
//!
//! 运行方式（隔离根已准备好时）：
//! ```powershell
//! $root = Join-Path $env:TEMP ('tokenscope-native-' + [guid]::NewGuid().ToString('N'))
//! .\scripts\prepare-native-acceptance.ps1 -Root $root
//! $env:TOKENSCOPE_ACCEPTANCE_ROOT = $root
//! cargo test --test native_acceptance_attribution -- --ignored --nocapture
//! ```
//! 未设置 `TOKENSCOPE_ACCEPTANCE_ROOT` 时跳过（不伪造通过）。

use std::path::PathBuf;

use tokenscope::aggregate::GroupBy;
use tokenscope::query;
use tokenscope::report::{EventFilter, SummaryOptions, summary};

const TOKENSCOPE_KEY: &str = "C:/acceptance/workspace/tokenscope";
const BEE_KEY: &str = "C:/acceptance/workspace/bee";

fn accept_root() -> Option<PathBuf> {
    let raw = std::env::var("TOKENSCOPE_ACCEPTANCE_ROOT").ok()?;
    let p = PathBuf::from(raw.trim());
    if p.is_absolute() && p.is_dir() {
        Some(p)
    } else {
        None
    }
}

#[test]
#[ignore = "需要先运行 scripts/prepare-native-acceptance.ps1 并设置 TOKENSCOPE_ACCEPTANCE_ROOT"]
fn acceptance_root_project_attribution() {
    let Some(root) = accept_root() else {
        eprintln!("未设置 TOKENSCOPE_ACCEPTANCE_ROOT（或不是目录），跳过");
        return;
    };
    let data = root.join("tokenscope");
    let opts = SummaryOptions {
        by: GroupBy::Project,
        claude_dir: Some(root.join("sources").join("claude")),
        codex_dir: Some(root.join("sources").join("codex")),
        cache_dir: Some(data.clone()),
        pricing_index: Some(data.join("pricing-index.json")),
        pricing_path: Some(data.join("pricing.toml")),
        openrouter_path: Some(data.join("openrouter-missing.json")),
        modelsdev_path: Some(data.join("modelsdev-missing.json")),
        tz: Some("Asia/Shanghai".to_string()),
        ..Default::default()
    };
    let r = summary(&opts).unwrap();
    let keys: Vec<String> = r.groups.iter().map(|g| g.key.clone()).collect();

    // 同路径跨工具合并：一行、两侧 agent。
    let merged = r
        .groups
        .iter()
        .find(|g| g.key == TOKENSCOPE_KEY)
        .unwrap_or_else(|| panic!("缺少合并项目行：{keys:?}"));
    let mut agents = merged.agents.clone();
    agents.sort();
    assert_eq!(agents, ["claude-code", "codex"], "合并行必须含两侧 agent");
    assert_eq!(
        merged.label.as_deref(),
        Some("tokenscope"),
        "展示名 = 路径末段"
    );

    // 越界切换成新项目，其子目录归并进来。
    let bee = r
        .groups
        .iter()
        .find(|g| g.key == BEE_KEY)
        .unwrap_or_else(|| panic!("缺少切换后的新项目行：{keys:?}"));
    assert_eq!(bee.agents, ["claude-code"]);

    // 子目录不得独立成行，也不得残留 slug 身份。
    for banned in [
        "src",
        "C:/acceptance/workspace/bee/123",
        "-acceptance-workspace-tokenscope",
    ] {
        assert!(
            !keys.iter().any(|k| k == banned),
            "不得出现独立身份 {banned}：{keys:?}"
        );
    }

    // 下钻：合并项目里两侧事件都在；切换前的请求不被回溯改写。
    let snap = query::begin_query(&opts).unwrap();
    let l = query::query_events(
        &snap.query_id,
        &EventFilter {
            project: Some(TOKENSCOPE_KEY.to_string()),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(l.total >= 2, "合并项目至少含两侧各一条事件");
    let mut seen: Vec<&str> = l.rows.iter().map(|r| r.agent).collect();
    seen.sort();
    seen.dedup();
    assert_eq!(seen, ["claude-code", "codex"]);
    assert!(l.rows.iter().all(|r| r.project == TOKENSCOPE_KEY));
}
