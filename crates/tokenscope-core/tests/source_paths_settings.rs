//! PR3 S01（计划 §6/§9.2）：来源目录集合与迁移的配置契约。
//!
//! 覆盖：旧空/非空/停用 dir、新旧并存、显式空列表、幂等、原子失败、并发事务、
//! 父根迁移重叠说明、手动父/子拒绝（含停用与缺失行）、遗留 JSON、其他字段逐字段
//! 保留。所有路径都在临时目录并注入主目录/cwd，不触真实主目录与环境变量。
//!
//! 边界（§9.2 第 7 条）：本文件只固化配置与预期；旧自定义 dir 与默认根的
//! **升级用量并集断言**在 S03 接通多根采集后于 `source_paths_collection.rs`
//! 执行，不在此提前关闭集成验收。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use tokenscope::settings::{
    AdditionalPathReject, AdditionalSourcePath, SETTINGS_FORMAT_REVISION, Settings,
    SourceMigrationContext, check_additional_path, load, migrate_source_paths_with, update,
    validate_additional_path_shape,
};

static SEQ: AtomicU64 = AtomicU64::new(0);

fn temp_dir(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "tokenscope-sp-settings-{tag}-{}-{n}",
        std::process::id(),
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

struct Fixture {
    root: PathBuf,
    path: PathBuf,
    ctx: SourceMigrationContext,
}

impl Fixture {
    fn new(tag: &str) -> Self {
        let root = temp_dir(tag);
        let home = root.join("home");
        let cwd = root.join("cwd");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&cwd).unwrap();
        Self {
            path: root.join("settings.toml"),
            root,
            ctx: SourceMigrationContext {
                home,
                cwd: cwd.clone(),
            },
        }
    }

    fn write(&self, text: &str) {
        std::fs::write(&self.path, text).unwrap();
    }

    fn read(&self) -> String {
        std::fs::read_to_string(&self.path).unwrap()
    }

    fn load(&self) -> Settings {
        load(&self.path).unwrap()
    }

    fn migrate(&self) -> tokenscope::settings::SourcePathMigration {
        migrate_source_paths_with(&self.path, &self.ctx).unwrap()
    }

    fn cleanup(&self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.cleanup();
    }
}

fn claude_cfg(s: &Settings) -> &tokenscope::settings::SourceConfig {
    s.sources.claude.as_ref().unwrap()
}

// ── 迁移：旧 dir 的四种形态 ────────────────────────────────────────

/// §6 S01「旧空」：空 dir 归一为新格式，不产生空路径行。
#[test]
fn legacy_empty_dir_migrates_to_new_format_without_additional_entry() {
    let fx = Fixture::new("empty-dir");
    fx.write("[sources.claude]\nenabled = true\ndir = \"\"\n");
    let out = fx.migrate();
    assert!(
        out.changed,
        "空 dir 也要完成格式归一（否则旧语义把空串当根）"
    );
    assert!(out.converted.is_empty(), "旧空值无追加项");
    let s = fx.load();
    let cfg = claude_cfg(&s);
    assert!(!cfg.is_legacy_dir_semantics());
    assert!(cfg.enabled);
    assert!(cfg.auto_discover);
    assert!(cfg.additional_paths.is_empty());
    let text = fx.read();
    assert!(!text.contains("dir"), "迁移后不再输出 dir: {text}");
    assert!(!text.contains("\"\""), "不得留下空路径: {text}");
}

/// §6 S01「旧非空」：dir → 一个追加项 + 自动发现开启 + dir 消失。
#[test]
fn legacy_nonempty_dir_migrates_to_additional_path_and_enables_auto_discover() {
    let fx = Fixture::new("nonempty-dir");
    fx.write("[sources.claude]\nenabled = true\ndir = \"D:/history/claude/projects\"\n");
    let out = fx.migrate();
    assert!(out.changed);
    assert_eq!(out.converted.len(), 1);
    let conv = &out.converted[0];
    assert_eq!(conv.agent, "claude");
    assert_eq!(conv.old_dir, "D:/history/claude/projects");
    assert_eq!(conv.path, "D:/history/claude/projects");
    assert!(conv.enabled);
    assert!(out.overlap_notes.is_empty(), "非主目录根不提示重叠");
    let s = fx.load();
    let cfg = claude_cfg(&s);
    assert_eq!(
        cfg.additional_paths,
        vec![AdditionalSourcePath {
            path: "D:/history/claude/projects".into(),
            enabled: true,
        }]
    );
    assert!(cfg.auto_discover, "自动发现开启（读取范围行为变更的载体）");
    let text = fx.read();
    assert!(
        text.contains("[[sources.claude.additional_paths]]"),
        "{text}"
    );
    assert!(!text.contains("dir"), "迁移后不再输出 dir: {text}");
    // 新格式只输出 enabled/auto_discover/excluded_auto_paths/additional_paths
    assert!(text.contains("auto_discover = true"), "{text}");
    assert!(text.contains("excluded_auto_paths = []"), "{text}");
}

/// §6 S01「旧停用」：enabled=false 保留（迁移不改启停；不开始采集由采集侧保证）。
#[test]
fn legacy_disabled_source_keeps_disabled_and_migrates_entry() {
    let fx = Fixture::new("disabled-dir");
    fx.write("[sources.codex]\nenabled = false\ndir = \"D:/history/codex/sessions\"\n");
    let out = fx.migrate();
    assert!(out.changed);
    assert_eq!(out.converted.len(), 1);
    assert!(!out.converted[0].enabled, "追加项继承旧来源停用值");
    let s = fx.load();
    let cfg = s.sources.codex.as_ref().unwrap();
    assert!(!cfg.enabled, "原已停用来源保持停用");
    assert_eq!(cfg.additional_paths.len(), 1);
}

/// §9.2 第 2 条「新旧并存」：任一新格式字段出现即按新配置解释，旧 dir 忽略且
/// 不迁移（文件字节不动）。
#[test]
fn new_format_fields_present_means_dir_ignored_and_no_migration() {
    let fx = Fixture::new("mixed-fields");
    fx.write("[sources.claude]\nenabled = true\ndir = \"D:/x\"\nauto_discover = false\n");
    let before = fx.read();
    let out = fx.migrate();
    assert!(!out.changed, "新格式在位时不得改写文件");
    assert!(out.converted.is_empty());
    assert_eq!(fx.read(), before, "文件字节必须不变");
    let s_loaded = fx.load();
    let cfg = claude_cfg(&s_loaded);
    assert!(!cfg.is_legacy_dir_semantics(), "按新格式解释");
    assert!(!cfg.auto_discover);
    assert!(cfg.additional_paths.is_empty(), "旧 dir 不补入追加项");
}

/// §6 S01「空数组」：显式 `additional_paths = []` 是新格式，不迁移、不猜旧值。
#[test]
fn explicit_empty_additional_paths_is_new_format_and_not_migrated() {
    let fx = Fixture::new("explicit-empty");
    fx.write("[sources.claude]\nenabled = true\ndir = \"D:/x\"\nexcluded_auto_paths = []\n");
    let before = fx.read();
    let out = fx.migrate();
    assert!(!out.changed);
    assert_eq!(fx.read(), before);
    let s_loaded = fx.load();
    let cfg = claude_cfg(&s_loaded);
    assert!(!cfg.is_legacy_dir_semantics());
    assert!(cfg.additional_paths.is_empty());
    assert_eq!(cfg.excluded_auto_paths, Vec::<String>::new());
}

/// §6 S01「幂等」：第二次迁移不再改写；load 永不写盘（不把缺 dir 当“又迁一次”）。
#[test]
fn migration_is_idempotent_and_load_never_rewrites() {
    let fx = Fixture::new("idempotent");
    fx.write("[sources.claude]\nenabled = true\ndir = \"D:/history/claude/projects\"\n");
    assert!(fx.migrate().changed);
    let after_first = fx.read();
    let second = fx.migrate();
    assert!(!second.changed, "重复加载/迁移不得再次改写");
    assert!(second.converted.is_empty());
    assert_eq!(fx.read(), after_first);
}

/// §9.2 第 3 条：迁移保存失败 → 明确错误、原文件字节不变、不回退成默认启用。
/// 用 Windows 共享锁（FILE_SHARE_READ）占住目标文件使原子替换失败。
#[test]
#[cfg(windows)]
fn migration_save_failure_keeps_original_bytes_unchanged() {
    use std::os::windows::fs::OpenOptionsExt;
    let fx = Fixture::new("save-failure");
    let original = "price_auto_sync = false\n[sources.claude]\nenabled = false\ndir = \"D:/history/claude/projects\"\n";
    fx.write(original);
    let _guard = std::fs::OpenOptions::new()
        .read(true)
        // FILE_SHARE_READ：迁移的读取仍可进行，但原子替换（需要 DELETE 权限）失败
        .share_mode(1)
        .open(&fx.path)
        .unwrap();
    let err = migrate_source_paths_with(&fx.path, &fx.ctx).unwrap_err();
    assert!(
        err.to_string().contains("原子替换失败") || err.to_string().contains("写设置失败"),
        "错误应可读: {err}"
    );
    drop(_guard);
    assert_eq!(fx.read(), original, "失败时文件字节必须不变");
    let s = fx.load();
    let cfg = claude_cfg(&s);
    assert!(
        cfg.is_legacy_dir_semantics(),
        "失败后仍按旧语义，不扩大读取范围"
    );
    assert!(!s.price_auto_sync, "其他字段未被默认值覆盖");
}

/// §9.2 第 5 条：除 sources 外的所有字段逐字段保留（含另一应用配置）。
#[test]
fn migration_preserves_every_other_field() {
    let fx = Fixture::new("preserve-fields");
    fx.write(
        "privacy_policy_accepted = true\n\
         price_auto_sync = false\n\
         close_action = \"minimize\"\n\
         timezone = \"UTC\"\n\
         collect_interval_secs = 30\n\
         [sources.claude]\nenabled = true\ndir = \"D:/history/claude/projects\"\n\
         [sources.codex]\nenabled = false\n",
    );
    fx.migrate();
    let s = fx.load();
    assert!(s.privacy_policy_accepted);
    assert!(!s.price_auto_sync);
    assert_eq!(
        s.close_action,
        Some(tokenscope::settings::CloseAction::Minimize)
    );
    assert_eq!(s.timezone.as_deref(), Some("UTC"));
    assert_eq!(s.collect_interval_secs, Some(30));
    assert!(
        !s.sources.codex.as_ref().unwrap().enabled,
        "另一应用配置保留"
    );
    assert!(claude_cfg(&s).auto_discover);
}

/// §6 S01「并发事务」：迁移与 update 共享同一把写锁，交错执行后两边改动都在
/// （确定性握手：不依赖顺序，只断言合并结果）。
#[test]
fn migration_and_update_concurrently_keep_all_fields() {
    let fx = Fixture::new("concurrent");
    fx.write("[sources.claude]\nenabled = true\ndir = \"D:/history/claude/projects\"\n");
    let path = fx.path.clone();
    let ctx = fx.ctx.clone();
    let migrator = std::thread::spawn(move || migrate_source_paths_with(&path, &ctx).unwrap());
    let path2 = fx.path.clone();
    let updater = std::thread::spawn(move || {
        update(&path2, |s| {
            s.timezone = Some("UTC".into());
            Ok(())
        })
        .unwrap()
    });
    migrator.join().unwrap();
    updater.join().unwrap();
    let s = fx.load();
    assert_eq!(
        s.timezone.as_deref(),
        Some("UTC"),
        "update 的改动不得被迁移覆盖"
    );
    let cfg = claude_cfg(&s);
    assert!(
        cfg.auto_discover && cfg.additional_paths.len() == 1,
        "迁移结果不得被 update 丢失"
    );
}

/// 回归（完整门禁发现的真实竞态）：update 对**未迁移** v1 文件做无关修改时，
/// 落盘不得带出 `auto_discover`——派生序列化会把文件混合化（dir+auto_discover），
/// 下次读取按新格式解释、旧 dir 被静默忽略，读取范围在迁移失败窗口内悄悄改变。
/// update 后文件必须仍是 v1 形态，且后续迁移照常把 dir 转为追加项。
#[test]
fn update_on_legacy_file_keeps_v1_shape_and_migration_still_converts() {
    let fx = Fixture::new("update-legacy-shape");
    fx.write("[sources.claude]\nenabled = true\ndir = \"D:/history/claude/projects\"\n");
    update(&fx.path, |s| {
        s.timezone = Some("UTC".into());
        Ok(())
    })
    .unwrap();
    let text = fx.read();
    assert!(
        !text.contains("auto_discover"),
        "update 不得把 v1 文件混合化: {text}"
    );
    assert!(text.contains("dir"), "v1 dir 必须原样保留: {text}");
    assert!(
        fx.load().sources.claude.unwrap().is_legacy_dir_semantics(),
        "重载仍按 v1 语义解释"
    );
    let out = fx.migrate();
    assert!(out.changed && out.converted.len() == 1, "迁移照常进行");
    let s = fx.load();
    assert_eq!(s.timezone.as_deref(), Some("UTC"), "无关修改必须保留");
    let cfg = claude_cfg(&s);
    assert!(
        cfg.auto_discover && cfg.additional_paths.len() == 1,
        "迁移结果不得被 update 丢失"
    );
}

/// §9.2 第 4 条「相对 dir 已存在」：按旧入口语义（cwd）转绝对追加路径。
/// 身份比较用 canonicalize：实现经 normalize_path（canonicalize 优先）会把
/// 8.3 短名（CI runner 的 RUNNER~1）解析为长名，按字面字符串比较在含短名
/// 的 TEMP 下必然失配（PR #14 CI 实测），属测试断言环境脆弱，不是实现缺陷。
#[test]
fn legacy_relative_dir_existing_becomes_absolute_additional_path() {
    let fx = Fixture::new("relative-ok");
    let logs = fx.ctx.cwd.join("logs").join("claude");
    std::fs::create_dir_all(&logs).unwrap();
    fx.write("[sources.claude]\nenabled = true\ndir = \"logs/claude\"\n");
    let out = fx.migrate();
    assert_eq!(out.converted.len(), 1);
    let same_dir =
        |p: &str| std::fs::canonicalize(p).unwrap() == std::fs::canonicalize(&logs).unwrap();
    assert!(
        same_dir(&out.converted[0].path),
        "相对 dir 必须按 cwd 解释为同一绝对目录（got {}）",
        out.converted[0].path
    );
    let s = fx.load();
    let cfg = claude_cfg(&s);
    assert!(
        same_dir(&cfg.additional_paths[0].path),
        "落盘追加路径必须指向同一目录（got {}）",
        cfg.additional_paths[0].path
    );
}

/// §9.2 第 4 条「相对 dir 不存在」：可操作错误 + 原文件保留，不默默扫默认根。
#[test]
fn legacy_relative_dir_missing_fails_and_keeps_file() {
    let fx = Fixture::new("relative-missing");
    let original = "[sources.claude]\nenabled = true\ndir = \"logs/no-such\"\n";
    fx.write(original);
    let err = migrate_source_paths_with(&fx.path, &fx.ctx).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("相对路径") && msg.contains("不存在"),
        "错误须可操作: {msg}"
    );
    assert!(msg.contains("sources.claude.dir"), "错误须指出字段: {msg}");
    assert_eq!(fx.read(), original, "失败时文件字节不变");
}

/// §6 S01「父根迁移重叠保留和说明」：旧根等于/包含自动根 → 迁移成功并提示重叠。
#[test]
fn migration_overlap_note_for_same_root_and_parent_root() {
    let fx = Fixture::new("overlap");
    let claude_same = fx.ctx.home.join(".claude").join("projects");
    let codex_parent = fx.ctx.home.join(".codex");
    fx.write(&format!(
        "[sources.claude]\nenabled = true\ndir = {}\n\
         [sources.codex]\nenabled = true\ndir = {}\n",
        toml_quote(&claude_same.display().to_string()),
        toml_quote(&codex_parent.display().to_string()),
    ));
    let out = fx.migrate();
    assert!(out.changed);
    assert_eq!(
        out.converted.len(),
        2,
        "重叠仍迁移成功（采集层保证只解析一次）"
    );
    assert_eq!(
        out.overlap_notes.len(),
        2,
        "两条重叠提示: {:?}",
        out.overlap_notes
    );
    assert!(
        out.overlap_notes.iter().any(|n| n.contains("相同")),
        "{:?}",
        out.overlap_notes
    );
    assert!(
        out.overlap_notes.iter().any(|n| n.contains("嵌套")),
        "{:?}",
        out.overlap_notes
    );
    let s = fx.load();
    assert!(claude_cfg(&s).auto_discover);
    assert!(s.sources.codex.as_ref().unwrap().auto_discover);
}

fn toml_quote(s: &str) -> String {
    format!("\"{s}\"").replace('\\', "\\\\")
}

/// §9.2 第 1 条：旧 settings.json（含 dir）同样被迁移收敛到新格式 toml。
#[test]
fn legacy_json_with_dir_migrates_to_new_format_toml() {
    let fx = Fixture::new("legacy-json");
    std::fs::write(
        fx.path.parent().unwrap().join("settings.json"),
        r#"{"sources": {"claude": {"enabled": true, "dir": "D:/history/claude/projects"}}}"#,
    )
    .unwrap();
    let out = fx.migrate();
    assert!(out.changed);
    assert_eq!(out.converted.len(), 1);
    let text = fx.read();
    assert!(
        text.contains("[[sources.claude.additional_paths]]"),
        "{text}"
    );
    assert!(
        !fx.path.parent().unwrap().join("settings.json").exists(),
        "迁移落 toml 后遗留 json 必须改名"
    );
}

// ── 追加路径校验（保存事务用，S04 接线）────────────────────────────

/// §5：拒绝空值、相对路径、整盘根、整个用户主目录。
#[test]
fn additional_path_shape_rejects_empty_relative_root_and_home() {
    let home = Path::new("C:/Users/someone");
    assert!(validate_additional_path_shape("   ", home).is_err());
    assert!(validate_additional_path_shape("", home).is_err());
    let rel = validate_additional_path_shape("logs/claude", home).unwrap_err();
    assert!(rel.contains("绝对路径"), "{rel}");
    for root in ["C:/", "C:\\\\", "/"] {
        let err = validate_additional_path_shape(root, home).unwrap_err();
        assert!(err.contains("整盘"), "{root}: {err}");
    }
    let home_err = validate_additional_path_shape("C:/Users/SOMEONE", home).unwrap_err();
    assert!(home_err.contains("主目录"), "{home_err}");
    // 合法缺失的绝对根可保存（存在与否由发现层显示）。
    assert_eq!(
        validate_additional_path_shape("  D:/history/claude/projects  ", home).unwrap(),
        PathBuf::from("D:/history/claude/projects")
    );
}

/// §4.1：同应用父/子拒绝——已停用与缺失行同样参与比较；同根提示已有行。
#[test]
fn additional_path_conflict_rejects_same_and_nested_even_when_disabled_or_missing() {
    let disabled = PathBuf::from("D:/history/disabled-root");
    let missing = PathBuf::from("D:/history/missing-root");
    let active = PathBuf::from("D:/history/active-root");
    let existing = vec![disabled.clone(), missing.clone(), active.clone()];
    // 同根（含大小写与分隔符别名）→ AlreadyExists。
    assert_eq!(
        check_additional_path(&existing, Path::new("d:/history/DISABLED-ROOT/")).unwrap_err(),
        AdditionalPathReject::AlreadyExists {
            existing: disabled.display().to_string()
        }
    );
    // 子目录/父目录 → Nested（含停用行与缺失行）。
    assert_eq!(
        check_additional_path(&existing, Path::new("D:/history/missing-root/sub")).unwrap_err(),
        AdditionalPathReject::Nested {
            existing: missing.display().to_string()
        }
    );
    assert_eq!(
        check_additional_path(&existing, Path::new("D:/history")).unwrap_err(),
        AdditionalPathReject::Nested {
            existing: disabled.display().to_string()
        }
    );
    // 组件边界：C:/ab 不是 C:/a 的子目录，不误报。
    assert!(check_additional_path(&[PathBuf::from("C:/a")], Path::new("C:/ab")).is_ok());
    // 无冲突 → 通过。
    assert!(check_additional_path(&existing, Path::new("E:/other/logs")).is_ok());
}

// ── 升级用例固化（用量并集断言待 S03）────────────────────────────

/// §6 S01 升级用例（配置侧固化）：旧配置只读自定义 dir；迁移后配置进入
/// "默认根 + 自定义根"追加语义。重复请求/独立用量的并集断言在
/// `source_paths_collection.rs`（S03）以相同场景执行完整验证。
#[test]
fn upgrade_old_dir_plus_default_root_fixture_config_expectations() {
    let fx = Fixture::new("upgrade-config");
    // 默认根与自定义根各准备一处日志位置（S03 在此放入含重复请求的合成用量）。
    let default_root = fx.ctx.home.join(".claude").join("projects");
    let custom_root = fx.root.join("custom").join("projects");
    std::fs::create_dir_all(default_root.join("p1")).unwrap();
    std::fs::create_dir_all(custom_root.join("p2")).unwrap();
    fx.write(&format!(
        "[sources.claude]\nenabled = true\ndir = {}\n",
        toml_quote(&custom_root.display().to_string())
    ));
    let out = fx.migrate();
    assert_eq!(out.converted.len(), 1);
    assert_eq!(out.converted[0].path, custom_root.display().to_string());
    let s = fx.load();
    let cfg = claude_cfg(&s);
    assert!(
        cfg.auto_discover,
        "迁移后默认根历史纳入读取（总量可能增加）"
    );
    assert_eq!(
        cfg.additional_paths[0].path,
        custom_root.display().to_string()
    );
    assert!(
        !cfg.is_legacy_dir_semantics(),
        "旧语义（只读自定义根）终止，由追加语义取代"
    );
}

/// 格式修订号钉住 v2：设置解释语义变更必须同步迁移规则（upgrade 契约）。
#[test]
fn settings_format_revision_is_2() {
    assert_eq!(SETTINGS_FORMAT_REVISION, 2);
}
