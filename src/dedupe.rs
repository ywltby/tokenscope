//! 全局去重（M4 自 source 层上移）：source 只产出未去重事件（保持发现顺序：
//! 文件排序 + 行序），这里按 agent 规则统一去重，保证「无缓存 / 缓存命中 /
//! --refresh」三条路径数字一致。

use std::collections::{HashMap, HashSet};

use crate::model::{AgentKind, UsageEvent};

/// Claude：同 `(session, message.id)` 保留时间戳最晚的一条（相同则保行序靠后）。
/// Codex：同 `(session, 用量五元组)` 保留首条（同请求重发逐字相同，首条即请求
/// 完成时刻；原始五元组可由归一化字段重建：raw_input = input + cache_read）。
///
/// 返回去重后事件（保持原相对顺序）与各 agent 的丢弃数（只列非零项）。
pub fn dedupe_events(events: Vec<UsageEvent>) -> (Vec<UsageEvent>, Vec<(AgentKind, u64)>) {
    let mut keep = vec![true; events.len()];
    let mut dropped: HashMap<AgentKind, u64> = HashMap::new();
    let mut claude_best: HashMap<(String, String), usize> = HashMap::new();
    let mut codex_seen: HashSet<(String, u64, u64, u64, u64)> = HashSet::new();

    for (i, e) in events.iter().enumerate() {
        match e.agent {
            AgentKind::ClaudeCode => {
                let key = (e.session_id.clone(), e.record_id.clone());
                match claude_best.get(&key) {
                    Some(&best) if e.ts < events[best].ts => {
                        keep[i] = false;
                        *dropped.entry(AgentKind::ClaudeCode).or_default() += 1;
                    }
                    Some(&best) => {
                        // 时间戳相同或更晚：行序靠后者胜
                        keep[best] = false;
                        *dropped.entry(AgentKind::ClaudeCode).or_default() += 1;
                        claude_best.insert(key, i);
                    }
                    None => {
                        claude_best.insert(key, i);
                    }
                }
            }
            AgentKind::Codex => {
                let key = (
                    e.session_id.clone(),
                    e.input_tokens,
                    e.output_tokens,
                    e.cache_write_tokens,
                    e.cache_read_tokens,
                );
                if codex_seen.insert(key) {
                    continue;
                }
                keep[i] = false;
                *dropped.entry(AgentKind::Codex).or_default() += 1;
            }
        }
    }

    let kept: Vec<UsageEvent> = events
        .into_iter()
        .zip(keep)
        .filter_map(|(e, k)| if k { Some(e) } else { None })
        .collect();
    let mut dropped: Vec<(AgentKind, u64)> = dropped.into_iter().collect();
    dropped.sort_by_key(|(a, _)| match a {
        AgentKind::ClaudeCode => 0,
        AgentKind::Codex => 1,
    });
    (kept, dropped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::Timestamp;

    struct E {
        agent: AgentKind,
        ts: &'static str,
        session: &'static str,
        record_id: &'static str,
        input: u64,
        output: u64,
        cw: u64,
        cr: u64,
    }

    impl Default for E {
        fn default() -> Self {
            Self {
                agent: AgentKind::ClaudeCode,
                ts: "2026-07-17T08:00:00Z",
                session: "s",
                record_id: "",
                input: 0,
                output: 0,
                cw: 0,
                cr: 0,
            }
        }
    }

    fn event(e: E) -> UsageEvent {
        UsageEvent {
            ts: e.ts.parse::<Timestamp>().unwrap(),
            agent: e.agent,
            model: "m".into(),
            session_id: e.session.into(),
            project: "p".into(),
            record_id: e.record_id.into(),
            input_tokens: e.input,
            output_tokens: e.output,
            cache_write_tokens: e.cw,
            cache_read_tokens: e.cr,
        }
    }

    #[test]
    fn test_dedupe_claude_keeps_last() {
        // 自 M1 fixture 单测迁移：三行同 id，保留时间戳最晚者。
        let events = vec![
            event(E {
                ts: "2026-07-17T08:00:00Z",
                session: "s1",
                record_id: "m1",
                input: 10,
                output: 10,
                ..Default::default()
            }),
            event(E {
                ts: "2026-07-17T08:00:01Z",
                session: "s1",
                record_id: "m1",
                input: 30,
                output: 30,
                ..Default::default()
            }),
            event(E {
                ts: "2026-07-17T08:00:00.500Z",
                session: "s1",
                record_id: "m1",
                input: 20,
                output: 20,
                ..Default::default()
            }),
            event(E {
                ts: "2026-07-17T08:00:02Z",
                session: "s1",
                record_id: "m2",
                input: 7,
                output: 7,
                ..Default::default()
            }),
        ];
        let (kept, dropped) = dedupe_events(events);
        assert_eq!(kept.len(), 2);
        assert_eq!(dropped, vec![(AgentKind::ClaudeCode, 2)]);
        assert_eq!(kept[0].input_tokens, 30, "保留时间戳最晚的 30/30 行");
        assert_eq!(kept[1].record_id, "m2");
    }

    #[test]
    fn test_dedupe_claude_same_ts_keeps_later_row() {
        let events = vec![
            event(E {
                ts: "2026-07-17T08:00:00Z",
                session: "s1",
                record_id: "m1",
                input: 10,
                output: 10,
                ..Default::default()
            }),
            event(E {
                ts: "2026-07-17T08:00:00Z",
                session: "s1",
                record_id: "m1",
                input: 20,
                output: 20,
                ..Default::default()
            }),
        ];
        let (kept, _) = dedupe_events(events);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].input_tokens, 20);
    }

    #[test]
    fn test_dedupe_claude_cross_file() {
        // 缓存场景核心：两个"文件批次"先后合并后仍全局保末条。
        let batch_a = vec![event(E {
            ts: "2026-07-17T10:00:00Z",
            session: "s1",
            record_id: "m1",
            input: 1,
            output: 1,
            ..Default::default()
        })];
        let batch_b = vec![event(E {
            ts: "2026-07-17T11:00:00Z",
            session: "s1",
            record_id: "m1",
            input: 2,
            output: 2,
            ..Default::default()
        })];
        let mut all = batch_a;
        all.extend(batch_b);
        let (kept, dropped) = dedupe_events(all);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].input_tokens, 2, "跨批次仍保留最晚时间戳");
        assert_eq!(dropped, vec![(AgentKind::ClaudeCode, 1)]);
    }

    #[test]
    fn test_dedupe_codex_keeps_first() {
        // 自 M2 fixture 单测迁移：同请求重发（数值相同、时间戳不同）保留首条。
        let events = vec![
            event(E {
                agent: AgentKind::Codex,
                ts: "2026-07-17T15:59:00Z",
                input: 800,
                output: 100,
                cw: 50,
                cr: 200,
                ..Default::default()
            }),
            event(E {
                agent: AgentKind::Codex,
                ts: "2026-07-17T15:59:05Z",
                input: 800,
                output: 100,
                cw: 50,
                cr: 200,
                ..Default::default()
            }),
            event(E {
                agent: AgentKind::Codex,
                ts: "2026-07-17T16:00:00Z",
                input: 10,
                output: 5,
                ..Default::default()
            }),
        ];
        let (kept, dropped) = dedupe_events(events);
        assert_eq!(kept.len(), 2);
        assert_eq!(dropped, vec![(AgentKind::Codex, 1)]);
        assert!(kept[0].ts < kept[1].ts, "首条在前");
    }

    #[test]
    fn test_dedupe_per_agent_isolated() {
        // 同名键在不同 agent 间互不影响。
        let events = vec![
            event(E {
                record_id: "x",
                input: 1,
                output: 1,
                ..Default::default()
            }),
            event(E {
                agent: AgentKind::Codex,
                ts: "2026-07-17T08:00:00Z",
                input: 1,
                output: 1,
                ..Default::default()
            }),
            event(E {
                agent: AgentKind::Codex,
                ts: "2026-07-17T08:00:01Z",
                input: 1,
                output: 1,
                ..Default::default()
            }),
        ];
        let (kept, dropped) = dedupe_events(events);
        assert_eq!(kept.len(), 2);
        assert_eq!(dropped, vec![(AgentKind::Codex, 1)]);
    }

    #[test]
    fn test_dedupe_empty() {
        let (kept, dropped) = dedupe_events(Vec::new());
        assert!(kept.is_empty());
        assert!(dropped.is_empty());
    }
}
