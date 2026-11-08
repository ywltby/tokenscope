//! 全局去重（M4 自 source 层上移）：source 只产出未去重事件（保持发现顺序：
//! 文件排序 + 行序），这里按 agent 规则统一去重，保证「无缓存 / 缓存命中 /
//! --refresh」三条路径数字一致。

use std::collections::HashMap;

use crate::model::{AgentKind, UsageEvent};

/// 去重结果。
#[derive(Debug, Default)]
pub struct DedupeResult {
    /// 去重后的事件（保持原有相对顺序）。
    pub events: Vec<UsageEvent>,
    /// 各 agent 的丢弃数（只列非零项）。
    pub dropped: Vec<(AgentKind, u64)>,
    /// 跨会话复制副本：`(保留事件在 [`Self::events`] 中的下标, 副本事件)`。
    ///
    /// 副本本身不贡献用量，但它的会话内身份与来源位置必须随保留事件一起写入
    /// 历史库——否则后续单文件增量重扫找不到旧事实，重复会复活（计划 §2.8）。
    pub copies: Vec<(usize, UsageEvent)>,
}

/// 跨会话副本的代表规则（固定、与文件发现顺序无关）：会话 ID 字典序最小，
/// 其次来源路径、行序。内容相同的副本任选其一都不改变用量，但展示的会话/
/// 项目必须稳定——不编造"父会话"，也不随扫描顺序翻转。
fn copy_representative_key(e: &UsageEvent) -> (&str, &str, u64) {
    (e.session_id.as_str(), e.source_path.as_str(), e.line)
}

/// Claude：同 `(session, message.id)` 保留时间戳最晚的一条（流式重发取终值）；
/// **相同时间**的不同终值候选（会话续写/分支产生的跨文件同刻候选）取确定性
/// 代表——四桶总量更大者，总量并列取 `(source_path, line)` 更大者——不再由
/// 行序或文件发现顺序决定（审阅 #14）。
/// Codex **强响应身份**（2026-11-08）：`token_usage_record` 带
/// `(thread_id, response_id)` 时按 `(thread, response_id)` 分组——不同响应
/// 即使用量完全相同也分别保留；同一响应的副本（复制历史/重播）只计一次，
/// 保留**事件时间最早**的一条（与弱记录的重播规则一致，副本不得把完成时间
/// 推后）。禁止按 turn_id 分组：一轮工具循环内可有多次模型调用。
/// Codex **弱记录**（缺身份旧格式）：同 `(session, model, 用量五元组)` 保留
/// **事件时间最早**的一条（首条即请求完成时刻；原始五元组可由归一化字段
/// 重建：raw_input = input + cache_read + cache_write）——重播副本/重发
/// 不得把完成时间推后，跨文件也不再依赖发现顺序（审阅 #15）；相同时间
/// 同上取确定性代表。
/// B2（R01）：弱键加入 model——不同模型同用量的真实请求不再误合并；
/// 同模型同用量的两个真实请求（弱记录）仍无法与重播区分，保守合并（限制见
/// docs/stats-semantics.md §3.2）。强/弱是两个键空间：不同强身份永远不能
/// 通过同一个弱指纹再次合并，强记录也不携带弱别名。
/// WB02：WorkBuddy 两应用同 `(session, messageId)` 强身份去重（messageId
/// 跨文件全局唯一），规则与 Claude 一致；键内含 agent——两应用的同一身份
/// 互不合并，也不触发跨应用去重。
///
/// 全局去重入口：按 agent 规则收敛请求事实，并带出跨会话副本的证据。
///
/// 两段：
/// 1. **会话内**：Claude/WorkBuddy 按 `(session, message.id)` 保留终值，
///    Codex 按响应身份（强）或保守重播族（弱）保留首条（规则与历史库写入
///    共用）；
/// 2. **跨会话复制**（仅 Claude，2026-11-07）：同一原始观察
///    （`message.id + requestId + uuid + 内容摘要`，见
///    [`UsageEvent::copy_identity`]）出现在多个会话时合并为一个代表。
///    证据缺失或内容矛盾时不合并——不按时间/用量猜测。
pub fn dedupe_events(events: Vec<UsageEvent>) -> DedupeResult {
    let mut keep = vec![true; events.len()];
    let mut dropped: HashMap<AgentKind, u64> = HashMap::new();
    // 强身份组（Claude + 两版 WorkBuddy + Codex 强响应）：键含 agent，
    // 应用间互不合并
    let mut strong_best: HashMap<(AgentKind, String, String), usize> = HashMap::new();
    // Codex 弱记录保守重播组：键不含时间——重播/重发与首条同族。
    // 仅缺强身份的旧格式记录进入本组；强/弱是两个键空间，互不吞并。
    let mut codex_best: HashMap<(String, String, u64, u64, u64, u64), usize> = HashMap::new();

    for (i, e) in events.iter().enumerate() {
        let mut strong = StrongUpsert {
            strong_best: &mut strong_best,
            keep: &mut keep,
            dropped: &mut dropped,
            events: &events,
        };
        match e.agent {
            AgentKind::ClaudeCode | AgentKind::WorkBuddy | AgentKind::WorkBuddyAi => {
                let key = (e.agent, e.session_id.clone(), e.record_id.clone());
                strong.upsert(strong_candidate_wins, key, i, e);
            }
            AgentKind::Codex if e.codex_response.is_some() => {
                // 强响应身份：`(thread, response_id)`——与 Claude 的
                // `(session, message.id)` 同形；不同响应即使用量完全相同
                // 也各自成组，同一响应的副本（复制历史/重播）命中同一组。
                // 禁止按 turn_id 分组：一轮工具循环内可有多次模型调用。
                let key = (e.agent, e.session_id.clone(), e.record_id.clone());
                strong.upsert(replay_candidate_wins, key, i, e);
            }
            AgentKind::Codex => {
                let key = (
                    e.session_id.clone(),
                    e.model.clone(),
                    e.input_tokens,
                    e.output_tokens,
                    e.cache_write_tokens,
                    e.cache_read_tokens,
                );
                match codex_best.get(&key).copied() {
                    Some(best) => {
                        if replay_candidate_wins(e, &events[best]) {
                            keep[best] = false;
                            *dropped.entry(AgentKind::Codex).or_default() += 1;
                            codex_best.insert(key, i);
                        } else {
                            keep[i] = false;
                            *dropped.entry(AgentKind::Codex).or_default() += 1;
                        }
                    }
                    None => {
                        codex_best.insert(key, i);
                    }
                }
            }
        }
    }

    // ── 第二段：跨会话复制合并 ─────────────────────────────────────
    // 复制身份自带内容摘要：同一标识但内容不同的记录得到不同身份，因此这里
    // 的分组本身就是"内容一致的副本"，不需要再做一次模糊匹配。
    let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, e) in events.iter().enumerate() {
        if !keep[i] {
            continue;
        }
        if let Some((_, value)) = e.copy_identity() {
            groups.entry(value).or_default().push(i);
        }
    }
    let mut merged = vec![false; events.len()];
    // (代表原下标, 副本事件)——副本内容要随保留事件一起写入来源证据。
    let mut copy_sources: Vec<(usize, UsageEvent)> = Vec::new();
    for (_, positions) in groups {
        if positions.len() < 2 {
            continue;
        }
        let rep = *positions
            .iter()
            .min_by(|a, b| {
                copy_representative_key(&events[**a]).cmp(&copy_representative_key(&events[**b]))
            })
            .expect("分组至少两条");
        for p in positions {
            if p == rep {
                continue;
            }
            merged[p] = true;
            copy_sources.push((rep, events[p].clone()));
            *dropped.entry(AgentKind::ClaudeCode).or_default() += 1;
        }
    }

    // 输出保留事件，并把副本映射到最终下标。
    let mut index_of = vec![usize::MAX; events.len()];
    let mut kept: Vec<UsageEvent> = Vec::with_capacity(events.len());
    for (i, e) in events.into_iter().enumerate() {
        if keep[i] && !merged[i] {
            index_of[i] = kept.len();
            kept.push(e);
        }
    }
    let copies: Vec<(usize, UsageEvent)> = copy_sources
        .into_iter()
        .map(|(rep, copy)| (index_of[rep], copy))
        .collect();
    let mut dropped: Vec<(AgentKind, u64)> = dropped.into_iter().collect();
    dropped.sort_by_key(|(a, _)| match a {
        AgentKind::ClaudeCode => 0,
        AgentKind::Codex => 1,
        AgentKind::WorkBuddy => 2,
        AgentKind::WorkBuddyAi => 3,
    });
    DedupeResult {
        events: kept,
        dropped,
        copies,
    }
}

/// 强身份组内插入或替换代表（Claude/WB 与 Codex 强响应的共用逻辑）：
/// 已有候选时按 `wins` 决定谁保留，无候选直接登记。
struct StrongUpsert<'a> {
    strong_best: &'a mut HashMap<(AgentKind, String, String), usize>,
    keep: &'a mut [bool],
    dropped: &'a mut HashMap<AgentKind, u64>,
    events: &'a [UsageEvent],
}

impl StrongUpsert<'_> {
    fn upsert(
        &mut self,
        wins: fn(&UsageEvent, &UsageEvent) -> bool,
        key: (AgentKind, String, String),
        i: usize,
        e: &UsageEvent,
    ) {
        match self.strong_best.get(&key).copied() {
            Some(best) => {
                if wins(e, &self.events[best]) {
                    self.keep[best] = false;
                    *self.dropped.entry(e.agent).or_default() += 1;
                    self.strong_best.insert(key, i);
                } else {
                    self.keep[i] = false;
                    *self.dropped.entry(e.agent).or_default() += 1;
                }
            }
            None => {
                self.strong_best.insert(key, i);
            }
        }
    }
}

/// 同刻候选的确定性代表/// 同刻候选的确定性代表：四桶总量更大者胜；总量并列取 `(source_path, line)`
/// 更大者（与历史库来源键 `native|app|路径|行序|指纹` 的字典序一致）。
fn same_instant_rep_wins(cand: &UsageEvent, best: &UsageEvent) -> bool {
    let (c_total, b_total) = (cand.total_tokens(), best.total_tokens());
    if c_total != b_total {
        return c_total > b_total;
    }
    (cand.source_path.as_str(), cand.line) > (best.source_path.as_str(), best.line)
}

/// Claude/WB 强身份：更晚事件时间胜出；相同时间取同刻代表。
fn strong_candidate_wins(cand: &UsageEvent, best: &UsageEvent) -> bool {
    if cand.ts != best.ts {
        return cand.ts > best.ts;
    }
    same_instant_rep_wins(cand, best)
}

/// Codex 保守重播：更早事件时间胜出（保首条 = 请求完成时刻）；相同时间取同刻代表。
fn replay_candidate_wins(cand: &UsageEvent, best: &UsageEvent) -> bool {
    if cand.ts != best.ts {
        return cand.ts < best.ts;
    }
    same_instant_rep_wins(cand, best)
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::Timestamp;

    /// 旧测试只看"保留 + 丢弃数"：副本证据另有专门用例（`copies`）。
    fn run(events: Vec<UsageEvent>) -> (Vec<UsageEvent>, Vec<(AgentKind, u64)>) {
        let r = dedupe_events(events);
        (r.events, r.dropped)
    }

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
            session_initial_cwd: None,
            event_cwd: None,
            record_id: e.record_id.into(),
            claude_copy: None,
            codex_response: None,
            codex_legacy_origin: None,
            line: 0,
            source_path: String::new(),
            input_tokens: e.input,
            output_tokens: e.output,
            cache_write_tokens: e.cw,
            cache_read_tokens: e.cr,
            cache_write_5m: 0,
            cache_write_1h: 0,
            cache_write_split_known: false,
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
        let (kept, dropped) = run(events);
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
        let (kept, _) = run(events);
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
        let (kept, dropped) = run(all);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].input_tokens, 2, "跨批次仍保留最晚时间戳");
        assert_eq!(dropped, vec![(AgentKind::ClaudeCode, 1)]);
    }

    #[test]
    fn test_dedupe_claude_same_instant_takes_deterministic_representative() {
        // 审阅 #14：同刻不同终值候选不靠行序——总量大者胜，总量并列取
        // (source_path, line) 更大者。两个输入顺序都收敛到同一结果。
        let make = |ts: &'static str,
                    session: &'static str,
                    record: &'static str,
                    input: u64,
                    path: &'static str| {
            let mut e = event(E {
                ts,
                session,
                record_id: record,
                input,
                output: 1,
                ..Default::default()
            });
            e.source_path = path.into();
            e
        };
        let small = make("2026-07-17T10:00:00Z", "s1", "m1", 10, "C:\\logs\\a.jsonl");
        let big = make("2026-07-17T10:00:00Z", "s1", "m1", 50, "C:\\logs\\b.jsonl");
        let tie_low = make("2026-07-17T10:00:00Z", "s1", "m1", 20, "C:\\logs\\a.jsonl");
        let mut tie_high = tie_low.clone();
        tie_high.line = 7;

        for order in [
            vec![small.clone(), big.clone()],
            vec![big.clone(), small.clone()],
        ] {
            let (kept, dropped) = run(order);
            assert_eq!(kept.len(), 1);
            assert_eq!(kept[0].input_tokens, 50, "同刻候选取四桶总量更大者");
            assert_eq!(dropped, vec![(AgentKind::ClaudeCode, 1)]);
        }
        let (kept, _) = run(vec![tie_low, tie_high]);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].line, 7, "总量并列取行序更大的来源记录");
    }

    #[test]
    fn test_dedupe_codex_replay_takes_earliest_time() {
        // 审阅 #15：重播副本不得把完成时间推后——同族候选取事件时间最早者，
        // 与到达顺序无关（旧规则保"发现顺序首条"，文件排序变化会翻转结果）。
        let make = |ts: &'static str, path: &'static str| {
            let mut e = event(E {
                agent: AgentKind::Codex,
                ts,
                session: "s1",
                input: 750,
                output: 100,
                cw: 50,
                cr: 200,
                ..Default::default()
            });
            e.source_path = path.into();
            e
        };
        let early = make("2026-07-17T15:59:00Z", "C:\\codex\\rollout-b.jsonl");
        let late = make("2026-07-18T16:08:00Z", "C:\\codex\\rollout-a.jsonl");
        for order in [vec![early.clone(), late.clone()], vec![late, early]] {
            let (kept, dropped) = run(order);
            assert_eq!(kept.len(), 1);
            assert_eq!(
                kept[0].ts.to_string(),
                "2026-07-17T15:59:00Z",
                "重播族取最早事件时间（首条完成时刻）"
            );
            assert_eq!(dropped, vec![(AgentKind::Codex, 1)]);
        }
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
        let (kept, dropped) = run(events);
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
        let (kept, dropped) = run(events);
        assert_eq!(kept.len(), 2);
        assert_eq!(dropped, vec![(AgentKind::Codex, 1)]);
    }

    #[test]
    fn test_dedupe_empty() {
        let (kept, dropped) = run(Vec::new());
        assert!(kept.is_empty());
        assert!(dropped.is_empty());
    }

    #[test]
    fn test_codex_replay_deduped() {
        // B2 双向 fixture 之重播侧：重播跨越"文件批次"边界仍去重（缓存合并
        // 场景：两个文件批次先后合并，同请求重发不能因分批而双计）。
        let batch_a = vec![event(E {
            agent: AgentKind::Codex,
            ts: "2026-07-17T15:59:00Z",
            session: "s1",
            input: 750,
            output: 100,
            cw: 50,
            cr: 200,
            ..Default::default()
        })];
        let batch_b = vec![event(E {
            agent: AgentKind::Codex,
            ts: "2026-07-17T16:08:00Z",
            session: "s1",
            input: 750,
            output: 100,
            cw: 50,
            cr: 200,
            ..Default::default()
        })];
        let mut all = batch_a;
        all.extend(batch_b);
        let (kept, dropped) = run(all);
        assert_eq!(kept.len(), 1, "跨批次重播只计一条");
        assert_eq!(dropped, vec![(AgentKind::Codex, 1)]);
    }

    #[test]
    fn test_dedupe_cross_day() {
        // B2：同会话重播跨本地日界（07-17 23:59Z 与次日 00:01Z 的请求），
        // 去重键不含日期——跨日重播仍合并，不因日界双计。
        let events = vec![
            event(E {
                agent: AgentKind::Codex,
                ts: "2026-07-17T15:59:00Z",
                session: "s1",
                input: 100,
                output: 10,
                ..Default::default()
            }),
            event(E {
                agent: AgentKind::Codex,
                ts: "2026-07-17T16:01:00Z",
                session: "s1",
                input: 100,
                output: 10,
                ..Default::default()
            }),
        ];
        let (kept, dropped) = run(events);
        assert_eq!(kept.len(), 1);
        assert_eq!(dropped, vec![(AgentKind::Codex, 1)]);
    }

    #[test]
    fn test_codex_distinct_requests_equal_usage() {
        // B2 双向 fixture 之"不同请求同用量"侧（R01）。
        let same_model = vec![
            event(E {
                agent: AgentKind::Codex,
                ts: "2026-07-17T15:59:00Z",
                session: "s1",
                input: 100,
                output: 10,
                ..Default::default()
            }),
            event(E {
                agent: AgentKind::Codex,
                ts: "2026-07-17T16:00:00Z",
                session: "s1",
                input: 100,
                output: 10,
                ..Default::default()
            }),
        ];
        // 同模型同用量：与重播不可区分（事件无请求身份），保守合并。
        // 已知限制，显式钉住；证据与替代方案见 docs/stats-semantics.md §3.2。
        let (kept, dropped) = run(same_model);
        assert_eq!(kept.len(), 1);
        assert_eq!(dropped, vec![(AgentKind::Codex, 1)]);

        // 不同模型同用量：B2 起键含 model，两条真实请求都保留（修复前误合并）。
        let mut a = event(E {
            agent: AgentKind::Codex,
            ts: "2026-07-17T15:59:00Z",
            session: "s1",
            input: 100,
            output: 10,
            ..Default::default()
        });
        a.model = "gpt-5.6-sol".into();
        let mut b = a.clone();
        b.model = "gpt-5.5".into();
        b.ts = "2026-07-17T16:00:00Z".parse().unwrap();
        let (kept, dropped) = run(vec![a, b]);
        assert_eq!(kept.len(), 2, "跨模型同用量不得合并");
        assert!(dropped.is_empty());
    }

    // ── 跨会话复制（2026-11-07） ───────────────────────────────────
    fn copied(session: &'static str) -> UsageEvent {
        let mut e = event(E {
            ts: "2026-09-15T08:00:00Z",
            session,
            record_id: "msg_01",
            input: 10,
            output: 2,
            cw: 3,
            cr: 4,
            ..Default::default()
        });
        e.claude_copy = Some(crate::model::ClaudeCopyEvidence {
            request_id: "req_01".into(),
            uuid: "uuid-01".into(),
        });
        e.source_path = format!("C:/logs/{session}.jsonl");
        e
    }

    #[test]
    fn copy_across_sessions_counts_once_and_keeps_evidence() {
        // 顺序无关：先原文件后副本、先副本后原文件都收敛到同一代表。
        for order in [
            vec![copied("s-a"), copied("s-b")],
            vec![copied("s-b"), copied("s-a")],
        ] {
            let r = dedupe_events(order);
            assert_eq!(r.events.len(), 1, "同一次调用的副本只留一条");
            assert_eq!(r.events[0].session_id, "s-a", "代表＝会话 ID 字典序最小");
            assert_eq!(r.dropped, vec![(AgentKind::ClaudeCode, 1)]);
            assert_eq!(r.copies.len(), 1, "副本的来源证据必须随保留事件带出");
            assert_eq!(r.copies[0].0, 0);
            assert_eq!(r.copies[0].1.session_id, "s-b");
        }
    }

    #[test]
    fn copy_without_evidence_or_with_other_content_is_not_merged() {
        // 缺 requestId/uuid：没有证据，不合并。
        let mut plain = copied("s-b");
        plain.claude_copy = None;
        let r = dedupe_events(vec![copied("s-a"), plain]);
        assert_eq!(r.events.len(), 2);
        assert!(r.copies.is_empty());

        // 同一标识但四桶不同：内容摘要不同，不合并。
        let mut contradictory = copied("s-b");
        contradictory.input_tokens = 99;
        let r = dedupe_events(vec![copied("s-a"), contradictory]);
        assert_eq!(r.events.len(), 2, "标识相同但内容矛盾不得合并");
        assert!(r.copies.is_empty());
    }

    #[test]
    fn streaming_observations_still_collapse_before_copy_matching() {
        // 同一会话内同 message.id 的两条流式观察先按终值收敛；终值与副本一致，
        // 跨会话再合并成一条（uuid 不单独成为请求身份）。
        let mut early = copied("s-a");
        early.ts = "2026-09-15T08:00:00Z".parse().unwrap();
        early.output_tokens = 1;
        let mut late = copied("s-a");
        late.ts = "2026-09-15T08:00:01Z".parse().unwrap();
        late.output_tokens = 2;
        late.claude_copy = Some(crate::model::ClaudeCopyEvidence {
            request_id: "req_01".into(),
            uuid: "uuid-02".into(),
        });
        let copy = late.clone();
        let mut copy_b = copy.clone();
        copy_b.session_id = "s-b".into();
        copy_b.source_path = "C:/logs/s-b.jsonl".into();

        let r = dedupe_events(vec![early, late, copy_b]);
        assert_eq!(r.events.len(), 1);
        assert_eq!(
            r.events[0].output_tokens, 2,
            "终值 2，不是把每条 UUID 当新请求"
        );
        assert_eq!(r.dropped, vec![(AgentKind::ClaudeCode, 2)]);
        assert_eq!(r.copies.len(), 1);
        assert_eq!(r.copies[0].1.session_id, "s-b");
    }
}
