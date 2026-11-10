//! F01：纠错备份只由实际纠错意图触发。
//!
//! 普通 Codex 强记录的重放/追加不能生成整库备份；真正的旧表示升级、同批
//! 多事实合并与弱 count-only 接管仍必须先备份。全部使用合成历史库与合成
//! 来源键，不读取真实会话、不写默认用户目录。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use tokenscope::history::{
    EventAlias, EventOrigin, EventWrite, HistoryDb, ORIGIN_KIND_NATIVE_FILE, PARSER_REVISION,
    WritePrecedence,
};
use tokenscope::model::{AgentKind, TokenCounts};
use tokenscope::model_identity::ModelIdentity;

const MODEL: &str = "gpt-5.1";
const TZ_TS: &str = "2026-11-10T10:00:00Z";

static SEQ: AtomicU64 = AtomicU64::new(0);

fn temp_dir(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "tokenscope-correction-policy-{tag}-{}-{n}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn db_path(dir: &Path) -> PathBuf {
    dir.join("data").join("history.db")
}

fn tokens() -> TokenCounts {
    TokenCounts {
        input: 100,
        output: 10,
        cache_write: 0,
        cache_read: 0,
        cache_write_5m: 0,
        cache_write_1h: 0,
        cache_write_split_known: false,
    }
}

fn origin(key: &str) -> EventOrigin {
    EventOrigin {
        origin_kind: ORIGIN_KIND_NATIVE_FILE.to_string(),
        origin_key: key.to_string(),
        app: AgentKind::Codex,
        import_run_id: None,
        parser_revision: PARSER_REVISION,
        source_model_raw: None,
        pricing_model: None,
        source_cost_usd: None,
        ts_precision_seconds: false,
    }
}

fn weak_alias(session: &str) -> EventAlias {
    EventAlias {
        app: AgentKind::Codex,
        scheme: "codex-usage".to_string(),
        value: format!("{session}|{MODEL}|100|10|0|0"),
    }
}

fn strong_alias(response: &str) -> EventAlias {
    EventAlias {
        app: AgentKind::Codex,
        scheme: "codex-response".to_string(),
        value: format!("thread-synthetic|{response}"),
    }
}

fn weak_write(session: &str, position: usize) -> EventWrite {
    let key = format!("native|codex|C:/synthetic/rollout.jsonl|{position}|weak-{position}");
    EventWrite {
        event_key: key.clone(),
        app: AgentKind::Codex,
        ts: TZ_TS.parse().unwrap(),
        model_raw: MODEL.to_string(),
        model_identity: ModelIdentity::parse(MODEL).identity_key(),
        session_id: Some(session.to_string()),
        record_id: None,
        project_key: Some("C:/synthetic/project".to_string()),
        session_initial_cwd: Some("C:/synthetic/project".to_string()),
        event_cwd: Some("C:/synthetic/project".to_string()),
        tokens: tokens(),
        precedence: WritePrecedence::NativeLog,
        observed_at: TZ_TS.parse().unwrap(),
        aliases: vec![weak_alias(session)],
        origins: vec![origin(&key)],
    }
}

/// 强响应写入：`mirror` 是旧解析器在该响应 count 行留下的镜像来源键。
fn strong_write(response: &str, position: usize, mirror: Option<&str>) -> EventWrite {
    let key = format!("native|codex|C:/synthetic/rollout.jsonl|{position}|strong-{response}");
    let mut origins = vec![origin(&key)];
    if let Some(mirror) = mirror {
        origins.push(origin(mirror));
    }
    EventWrite {
        event_key: key,
        app: AgentKind::Codex,
        ts: TZ_TS.parse().unwrap(),
        model_raw: MODEL.to_string(),
        model_identity: ModelIdentity::parse(MODEL).identity_key(),
        session_id: Some("session-synthetic".to_string()),
        record_id: Some(response.to_string()),
        project_key: Some("C:/synthetic/project".to_string()),
        session_initial_cwd: Some("C:/synthetic/project".to_string()),
        event_cwd: Some("C:/synthetic/project".to_string()),
        tokens: tokens(),
        precedence: WritePrecedence::NativeLog,
        observed_at: TZ_TS.parse().unwrap(),
        aliases: vec![strong_alias(response)],
        origins,
    }
}

fn correction_backups(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir.join("data"))
        .unwrap()
        .filter_map(|entry| {
            let path = entry.unwrap().path();
            let hit = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.contains("correction-backup"));
            hit.then_some(path)
        })
        .collect();
    out.sort();
    out
}

fn rows(dir: &Path) -> Vec<tokenscope::history::StoredEvent> {
    HistoryDb::open(&db_path(dir))
        .unwrap()
        .stored_events()
        .unwrap()
}

/// 普通强记录：首轮新增，之后同内容重放与追加新请求都不生成纠错备份。
#[test]
fn normal_codex_replay_and_append_do_not_create_correction_backups() {
    let dir = temp_dir("replay");
    {
        let db = HistoryDb::open(&db_path(&dir)).unwrap();
        let first = strong_write("resp-1", 1, None);
        let inserted = db
            .write_collected_batch(std::slice::from_ref(&first))
            .unwrap();
        assert_eq!(inserted.events.inserted, 1);
        assert_eq!(correction_backups(&dir).len(), 0, "正常新增不备份");

        let generation = db.generation().unwrap();
        for _ in 0..4 {
            let replay = db
                .write_collected_batch(std::slice::from_ref(&first))
                .unwrap();
            assert_eq!(replay.events.unchanged, 1, "强记录重放必须幂等");
        }
        assert_eq!(db.event_count().unwrap(), 1);
        assert_eq!(
            correction_backups(&dir).len(),
            0,
            "已是强身份的普通重放不得生成整库备份"
        );
        assert_eq!(db.generation().unwrap(), generation);

        let second = strong_write("resp-2", 2, None);
        let appended = db
            .write_collected_batch(std::slice::from_ref(&second))
            .unwrap();
        assert_eq!(appended.events.inserted, 1);
        assert_eq!(db.event_count().unwrap(), 2);
        assert_eq!(db.generation().unwrap(), generation + 1);
        for _ in 0..3 {
            db.write_collected_batch(std::slice::from_ref(&second))
                .unwrap();
        }
        assert_eq!(db.event_count().unwrap(), 2);
        assert_eq!(
            correction_backups(&dir).len(),
            0,
            "追加新请求后的重放同样不备份"
        );
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// 旧弱事实首次升为强响应：先备份，原行升级，token 不叠加；之后重放不重复备份。
#[test]
fn weak_to_strong_upgrade_backs_up_exactly_once() {
    let dir = temp_dir("upgrade");
    {
        let db = HistoryDb::open(&db_path(&dir)).unwrap();
        let weak = weak_write("session-synthetic", 0);
        let weak_key = weak.event_key.clone();
        db.write_batch(&[weak]).unwrap();
        assert_eq!(db.event_count().unwrap(), 1);
        assert_eq!(correction_backups(&dir).len(), 0);

        let strong = strong_write("resp-1", 1, Some(&weak_key));
        let upgraded = db
            .write_collected_batch(std::slice::from_ref(&strong))
            .unwrap();
        assert_eq!(upgraded.events.updated, 1);
        assert_eq!(db.event_count().unwrap(), 1, "原行升级，不新增事实");
        assert_eq!(correction_backups(&dir).len(), 1, "真实升级必须先备份");

        let stored = rows(&dir);
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].record_id.as_deref(), Some("resp-1"));
        assert_eq!(stored[0].tokens.total(), tokens().total());

        let generation = db.generation().unwrap();
        for _ in 0..3 {
            let replay = db
                .write_collected_batch(std::slice::from_ref(&strong))
                .unwrap();
            assert_eq!(replay.events.unchanged, 1);
        }
        assert_eq!(db.generation().unwrap(), generation);
        assert_eq!(correction_backups(&dir).len(), 1, "升级后的重放不再备份");
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// 同一批升级两条旧弱事实：只生成一份备份，两条都完成升级。
#[test]
fn batch_with_two_upgrades_creates_one_backup() {
    let dir = temp_dir("batch-upgrade");
    {
        let db = HistoryDb::open(&db_path(&dir)).unwrap();
        let weak_a = weak_write("session-a", 0);
        let weak_b = weak_write("session-b", 1);
        let key_a = weak_a.event_key.clone();
        let key_b = weak_b.event_key.clone();
        db.write_batch(&[weak_a, weak_b]).unwrap();
        assert_eq!(db.event_count().unwrap(), 2);

        let batch = [
            strong_write("resp-a", 2, Some(&key_a)),
            strong_write("resp-b", 3, Some(&key_b)),
        ];
        let out = db.write_collected_batch(&batch).unwrap();
        assert_eq!(out.events.updated, 2);
        assert_eq!(db.event_count().unwrap(), 2);
        assert_eq!(
            correction_backups(&dir).len(),
            1,
            "同一纠错批次最多一份备份"
        );
        let stored = rows(&dir);
        let mut ids: Vec<&str> = stored
            .iter()
            .filter_map(|row| row.record_id.as_deref())
            .collect();
        ids.sort_unstable();
        assert_eq!(ids, vec!["resp-a", "resp-b"]);
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// 弱 count-only 接管已升级行的旧弱来源/别名：仍属真实纠错，先备份再转移。
#[test]
fn weak_count_only_takeover_of_upgraded_row_backs_up() {
    let dir = temp_dir("takeover");
    {
        let db = HistoryDb::open(&db_path(&dir)).unwrap();
        let weak = weak_write("session-synthetic", 0);
        let weak_key = weak.event_key.clone();
        db.write_batch(&[weak]).unwrap();
        let strong = strong_write("resp-1", 1, Some(&weak_key));
        db.write_collected_batch(&[strong]).unwrap();
        assert_eq!(correction_backups(&dir).len(), 1);
        let upgraded = rows(&dir).pop().unwrap();
        assert_eq!(upgraded.record_id.as_deref(), Some("resp-1"));

        // 新 turn 后的同用量 count-only：来源位置仍指向已升级行，弱别名也
        // 仍绑定该行。当前解析表示是独立弱事实，必须转移证据而不是并入强行。
        let takeover = weak_write("session-synthetic", 2);
        let out = db.write_collected_batch(&[takeover]).unwrap();
        assert_eq!(out.events.inserted, 1);
        assert_eq!(db.event_count().unwrap(), 2);
        assert_eq!(
            correction_backups(&dir).len(),
            2,
            "弱 count-only 接管旧弱证据前必须另存备份"
        );
        let stored = rows(&dir);
        let weak_row = stored
            .iter()
            .find(|row| row.record_id.is_none())
            .expect("独立弱事实存在");
        let strong_row = stored
            .iter()
            .find(|row| row.record_id.as_deref() == Some("resp-1"))
            .unwrap();
        assert_ne!(weak_row.id, strong_row.id);
        assert_eq!(weak_row.tokens.total(), tokens().total());
        assert_eq!(strong_row.tokens.total(), tokens().total());
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// 备份失败：批次整体拒绝，旧事实/别名/来源/generation 全部不变。
#[test]
fn backup_failure_leaves_history_unchanged() {
    let dir = temp_dir("backup-failure");
    {
        let db = HistoryDb::open(&db_path(&dir)).unwrap();
        let weak = weak_write("session-synthetic", 0);
        let weak_key = weak.event_key.clone();
        db.write_batch(&[weak]).unwrap();
        let before = rows(&dir);
        let generation = db.generation().unwrap();
        let alias_count = db.alias_count().unwrap();
        let origin_count = db.origin_count().unwrap();

        // 让当前秒及随后数秒的全部候选备份路径都先被目录占用：备份函数在
        // 100 个后缀用尽后明确失败，且不改动数据库。
        let now = jiff::Timestamp::now();
        for offset in 0..10 {
            let stamp = (now + jiff::Span::new().seconds(offset))
                .strftime("%Y%m%dT%H%M%SZ")
                .to_string();
            for suffix in 0..100 {
                let name = if suffix == 0 {
                    format!("history.db.correction-backup-{stamp}")
                } else {
                    format!("history.db.correction-backup-{stamp}-{suffix}")
                };
                std::fs::create_dir_all(dir.join("data").join(name)).unwrap();
            }
        }

        let strong = strong_write("resp-1", 1, Some(&weak_key));
        let err = db.write_collected_batch(&[strong]).unwrap_err();
        assert!(
            err.to_string().contains("备份文件名已用尽") || err.to_string().contains("备份失败"),
            "{err}"
        );
        assert_eq!(rows(&dir), before, "旧事实逐字段不变");
        assert_eq!(db.generation().unwrap(), generation);
        assert_eq!(db.alias_count().unwrap(), alias_count);
        assert_eq!(db.origin_count().unwrap(), origin_count);
    }
    std::fs::remove_dir_all(&dir).ok();
}
