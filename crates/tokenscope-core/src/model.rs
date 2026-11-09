//! 归一化用量事件与 token 计数：source 层输出 `UsageEvent` 后，下游只认这里的类型。

use jiff::Timestamp;
use serde::Serialize;

/// 已接入的 agent 种类；新增适配器时在此扩枚举。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize)]
pub enum AgentKind {
    #[default]
    #[serde(rename = "claude-code")]
    ClaudeCode,
    #[serde(rename = "codex")]
    Codex,
    /// WorkBuddy 国内版（`~/.workbuddy`）。
    #[serde(rename = "workbuddy")]
    WorkBuddy,
    /// WorkBuddy AI 国际版（`~/.workbuddy-ai`）。
    #[serde(rename = "workbuddy-ai")]
    WorkBuddyAi,
}

impl AgentKind {
    /// 全部已接入应用（穷举点一律取自这里，新增应用时只改枚举与本表）。
    pub const ALL: [AgentKind; 4] = [
        AgentKind::ClaudeCode,
        AgentKind::Codex,
        AgentKind::WorkBuddy,
        AgentKind::WorkBuddyAi,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            AgentKind::ClaudeCode => "claude-code",
            AgentKind::Codex => "codex",
            AgentKind::WorkBuddy => "workbuddy",
            AgentKind::WorkBuddyAi => "workbuddy-ai",
        }
    }

    /// 持久层反向解析（库内不允许出现未知应用名——读到未知值必须报错，
    /// 不能悄悄当成默认应用）。
    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw {
            "claude-code" => Ok(AgentKind::ClaudeCode),
            "codex" => Ok(AgentKind::Codex),
            "workbuddy" => Ok(AgentKind::WorkBuddy),
            "workbuddy-ai" => Ok(AgentKind::WorkBuddyAi),
            other => Err(format!("未知应用标识: {other:?}")),
        }
    }
}

/// Claude 跨会话复制证据：上游原始请求标识与该行消息 UUID（2026-11-07）。
///
/// 同一次模型调用被复制到另一个会话时，副本会原样保留 `message.id`、
/// `requestId`、`uuid`、事件时间、模型与四桶，只有 `sessionId` 不同。三者
/// 齐备（且内容一致）才构成"这是同一条原始观察"的证据；单独 `message.id`
/// 或时间/用量相同都不足以合并不同会话的事实。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeCopyEvidence {
    pub request_id: String,
    pub uuid: String,
}

/// Codex 强响应身份（2026-11-08）：`token_usage_record` 的原始线程与响应
/// 标识。新格式 rollout 每次模型响应各有一个 `response_id`，`thread_id`
/// 指明响应所属线程——legacy fork 复制的父请求保留父线程身份，子线程的
/// 新请求归子线程，不受解析状态（复制历史遗留的 session_meta）影响。
/// 缺线程或缺响应 ID 的旧格式记录为 `None`，走弱身份兼容路径。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexResponseIdentity {
    pub thread_id: String,
    pub response_id: String,
}

/// 一条用量事件，agent 无关的归一形态（M4 起为**未去重**事件，去重在全局
/// dedupe 步骤按 agent 规则执行）。
#[derive(Debug, Clone)]
pub struct UsageEvent {
    pub ts: Timestamp,
    pub agent: AgentKind,
    pub model: String,
    pub session_id: String,
    /// 已决归属 key（阶段 B）：按"项目根 + 前缀归并"决定——进入当前根的
    /// 子目录仍归该根，越出当前根则视为切换到新项目。
    pub project: String,
    /// B02：会话**初始**工作目录（归一化）；None = 该会话没有可信 cwd，
    /// 身份由文件身份（Claude）或 `(未知)`（Codex）兜底。
    pub session_initial_cwd: Option<String>,
    /// B02：该请求作用域内最近观察到的结构化工作目录（归一化）；与
    /// `project` 的区别是它保留子目录细节，用于排查与后续规则演进。
    pub event_cwd: Option<String>,
    /// agent 原生日志标识（Claude 的 message.id；Codex 新格式为
    /// `token_usage_record.response_id`，旧格式无此标识记空串），
    /// 供全局去重使用。
    pub record_id: String,
    /// 跨会话复制证据（仅 Claude）：顶层 `requestId` 与消息 `uuid` 同时可用
    /// 时才有值。缺失、空白或类型异常一律为 `None`——旧日志照常解析，
    /// 只是没有跨会话证据（不猜测、不合并）。
    pub claude_copy: Option<ClaudeCopyEvidence>,
    /// Codex 强响应身份（2026-11-08）：`token_usage_record` 携带
    /// `thread_id + response_id` 时有值。会话归属以记录的原始线程为准
    /// （优先于复制历史遗留的解析状态）；`record_id` 同步保存 response_id。
    /// 旧格式（缺线程或缺响应 ID）为 `None`，保持弱身份兼容路径。
    pub codex_response: Option<CodexResponseIdentity>,
    /// Codex 旧事实升级映射（仅解析期）：本响应在**旧解析器表示**中的来源键
    /// （`native|codex|路径|配对 count 行|旧内容指纹`）。旧库中被弱指纹误合并
    /// 的事实经由该键精确命中并原行升级；键逐字节相等才建立映射，四桶与模型
    /// 等价只在命中后校验。缓存/历史恢复路径不携带（升级只发生在重解析轮）。
    pub codex_legacy_origin: Option<String>,
    /// H02：源文件内 0-based 行序——历史库 origin key 的组成部分，使一条
    /// 请求能被精确定位到"哪个文件的第几行"。无法得知时记 0。
    pub line: u64,
    /// H04：事件所属源文件的规范化路径（历史库的来源键组成部分）。
    /// 无文件来源（如外部汇总）记空串。
    pub source_path: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_write_tokens: u64,
    pub cache_read_tokens: u64,
    /// N18：缓存写总量中 5 分钟档的分项（仅 [`Self::cache_write_split_known`]
    /// 为 true 时有意义；未知时恒为 0，不得猜测）。
    pub cache_write_5m: u64,
    /// N18：缓存写总量中 1 小时档的分项（同上）。
    pub cache_write_1h: u64,
    /// N18：分项可信——来源日志同时给出 5m/1h 两键且 5m+1h == 写总量。
    /// false = 分项未知（旧日志、缺一键、类型异常或与总量不一致），
    /// 两分项字段必须为 0，计价按「分项未知」估算口径处理。
    pub cache_write_split_known: bool,
}

impl UsageEvent {
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens + self.cache_write_tokens + self.cache_read_tokens
    }

    /// SF08：单事件桶边界校验（source→model 公共边界，两适配器与缓存
    /// 恢复经过同一入口）——四桶总数与 prompt 计价基数必须可表示。
    /// 异常事件由调用方计 bad_lines 并跳过，不做回绕/饱和。
    /// N18：可信分档必须恰好等于写总量（分项是总量的内部构成，不得多计）。
    pub fn validate_buckets(&self) -> Result<(), &'static str> {
        let total = self
            .input_tokens
            .checked_add(self.output_tokens)
            .and_then(|v| v.checked_add(self.cache_write_tokens))
            .and_then(|v| v.checked_add(self.cache_read_tokens));
        if total.is_none() {
            return Err("四桶 token 总数超出可表示范围");
        }
        let prompt = self
            .input_tokens
            .checked_add(self.cache_write_tokens)
            .and_then(|v| v.checked_add(self.cache_read_tokens));
        if prompt.is_none() {
            return Err("prompt 计价基数超出可表示范围");
        }
        if self.cache_write_split_known {
            let split = self
                .cache_write_5m
                .checked_add(self.cache_write_1h)
                .ok_or("缓存写分项之和超出可表示范围")?;
            if split != self.cache_write_tokens {
                return Err("缓存写分项之和与写总量不一致");
            }
        } else if self.cache_write_5m != 0 || self.cache_write_1h != 0 {
            return Err("未知分档的事件不得携带分项数值");
        }
        Ok(())
    }

    /// H04：事件内容指纹（FNV-1a 64，跨进程稳定）。
    ///
    /// 用途：历史库的来源键 = 文件 + 行序 + **内容指纹**。这样
    /// - 同一行内容不变 → 同一来源键 → 完全相同的重复采集幂等跳过；
    /// - 同一行的内容变了（文件被替换、新请求占用同一行）→ 新来源键 →
    ///   新事件，**旧事实保留**，绝不用新值覆盖已保存的旧请求；
    /// - Claude 的流式终值更新由原生身份别名（`(session_id, message.id)`）
    ///   命中同一事件后按终值规则更新，不依赖来源键。
    pub fn content_fingerprint(&self) -> String {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut write = |bytes: &[u8]| {
            for b in bytes {
                h ^= *b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        };
        write(self.ts.to_string().as_bytes());
        write(b"\x1f");
        write(self.model.as_bytes());
        write(b"\x1f");
        write(self.session_id.as_bytes());
        write(b"\x1f");
        write(self.record_id.as_bytes());
        write(b"\x1f");
        write(self.source_path.as_bytes());
        for v in [
            self.input_tokens,
            self.output_tokens,
            self.cache_write_tokens,
            self.cache_read_tokens,
        ] {
            write(&v.to_le_bytes());
        }
        format!("{h:016x}")
    }

    /// 跨会话复制的**观察身份**（Claude 专用，2026-11-07）：
    /// `(message.id, requestId, 消息 uuid, 观察内容摘要)`。
    ///
    /// 只用可证实的三元组做证据，不按时间/用量猜测：同一次调用的副本会原样
    /// 保留三者，而分支后新发出的调用必然带新的 `requestId` / `uuid`。内容
    /// 摘要把"相同标识必须对应相同观察"写进身份本身——标识相同但模型、时间
    /// 或四桶不同的记录得到不同身份，**不会被当成同一次请求合并**。
    ///
    /// 返回 `(scheme, value)`；scheme 与 `claude-message` 并列保存在
    /// `event_aliases`，不改动会话内身份。
    pub fn copy_identity(&self) -> Option<(&'static str, String)> {
        if self.agent != AgentKind::ClaudeCode {
            return None;
        }
        let evidence = self.claude_copy.as_ref()?;
        if self.record_id.is_empty() || evidence.request_id.is_empty() || evidence.uuid.is_empty() {
            return None;
        }
        Some((
            "claude-copy",
            format!(
                "{}|{}|{}|{}",
                self.record_id,
                evidence.request_id,
                evidence.uuid,
                self.observation_digest()
            ),
        ))
    }

    /// 观察内容摘要（FNV-1a 64，跨进程稳定）：事件时间 + 模型 + 四桶 +
    /// 缓存写分档。**不含**会话、项目与文件位置——副本之间只有这些字段
    /// 相同才可能被认作同一条原始观察。
    pub fn observation_digest(&self) -> String {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut write = |bytes: &[u8]| {
            for b in bytes {
                h ^= *b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        };
        write(self.ts.to_string().as_bytes());
        write(b"\x1f");
        write(self.model.as_bytes());
        for v in [
            self.input_tokens,
            self.output_tokens,
            self.cache_write_tokens,
            self.cache_read_tokens,
            self.cache_write_5m,
            self.cache_write_1h,
        ] {
            write(b"\x1f");
            write(&v.to_le_bytes());
        }
        write(if self.cache_write_split_known {
            b"\x1fknown"
        } else {
            b"\x1funknown"
        });
        format!("{h:016x}")
    }

    /// H02：原生请求身份——跨来源（原生日志 ↔ CCS 导入）去重所依赖的
    /// **可证实**稳定标识。返回 `(scheme, value)`。
    ///
    /// - Claude：`(session_id, message.id)` 是上游稳定标识，同一请求的流式
    ///   更新与重复来源都指向同一条；
    /// - Codex（2026-11-08）：新格式 `token_usage_record` 带
    ///   `(thread_id, response_id)`，构成本线程内可证实的响应身份
    ///   （scheme 值含应用与身份版本，命名空间由值首段承担）；旧格式
    ///   rollout 的 `token_count` 记录不带请求标识，身份不足时返回
    ///   `None`——**不猜测身份**，该来源的请求只按库内来源键与既有保守
    ///   重播规则处理（跨来源合并需要的是可证实身份，故不给弱记录任何
    ///   跨来源别名）。
    pub fn native_identity(&self) -> Option<(&'static str, String)> {
        match self.agent {
            AgentKind::ClaudeCode => {
                if self.session_id.is_empty() || self.record_id.is_empty() {
                    None
                } else {
                    Some((
                        "claude-message",
                        format!("{}|{}", self.session_id, self.record_id),
                    ))
                }
            }
            AgentKind::Codex => self.codex_response_identity(),
            // WB02：WorkBuddy 转写带 providerData.messageId（跨文件全局唯一），
            // 身份方案与 Claude 同形（sessionId|messageId）；app 字段在库层
            // 区分国内版与国际版，两应用互不合并。
            AgentKind::WorkBuddy | AgentKind::WorkBuddyAi => {
                if self.session_id.is_empty() || self.record_id.is_empty() {
                    None
                } else {
                    Some((
                        "workbuddy-message",
                        format!("{}|{}", self.session_id, self.record_id),
                    ))
                }
            }
        }
    }

    /// 保守重播身份：文件被归档移动、复制成副本、或先经遗留缓存迁移后重扫时，
    /// 同一条请求必须仍映射到同一事件行——**与文件路径无关**。
    ///
    /// 规则与全局去重（`dedupe.rs` §3.2）完全一致：
    /// - Codex **强响应身份**（2026-11-08）：与
    ///   [`Self::native_identity`] 同一条 `codex-response` 别名——同一响应
    ///   的 record/count 两种观察、跨文件副本都命中同一身份；**不带**四桶，
    ///   不同响应即使用量相同也不会合并。
    /// - Codex **弱记录**（缺身份的旧格式）：`token_count` 没有请求标识，
    ///   `(session_id, 原始模型, 四桶)` 相同的记录按保守规则视为同一次请求
    ///   的重播/副本。CCS 导入对 Codex 行登记**同一个**别名
    ///   （`import::ccs::request_write`），使"先导入后采集"与"先采集后导入"
    ///   命中同一事件、得到相同并集；这是沿用既有保守规则（同模型同用量的
    ///   独立请求可能被合并），不是宣称可证实的强跨来源身份——缺 session_id
    ///   的 CCS 行仍按身份不足处理（重叠候选）。
    /// - 强/弱是两条不同路径：强记录绝不携带弱别名（不同强身份之间不能通过
    ///   同一个弱别名再次合并），弱记录也不伪造成强身份。
    pub fn same_source_identity(&self) -> Option<(&'static str, String)> {
        match self.agent {
            AgentKind::ClaudeCode => self.native_identity(),
            AgentKind::Codex => {
                if let Some(identity) = self.codex_response_identity() {
                    return Some(identity);
                }
                if self.session_id.is_empty() || self.model.is_empty() {
                    return None;
                }
                Some((
                    "codex-usage",
                    format!(
                        "{}|{}|{}|{}|{}|{}",
                        self.session_id,
                        self.model,
                        self.input_tokens,
                        self.output_tokens,
                        self.cache_write_tokens,
                        self.cache_read_tokens
                    ),
                ))
            }
            // WB02：与 native_identity 同方案（messageId 全局唯一，强身份）
            AgentKind::WorkBuddy | AgentKind::WorkBuddyAi => self.native_identity(),
        }
    }

    /// Codex 强响应身份（仅 Codex 且证据齐备时）：`(thread_id, response_id)`
    /// 加应用/版本命名空间。解析期以记录的原始线程为准；缓存/历史恢复路径
    /// 不携带解析期结构，从 `record_id` 派生（解析器只为强记录填写
    /// record_id，弱记录恒为空串；强记录的 session_id 即原始线程）。
    /// 缺响应 ID 或会话为空返回 `None`。
    fn codex_response_identity(&self) -> Option<(&'static str, String)> {
        let thread = match self.codex_response.as_ref() {
            Some(response) => response.thread_id.as_str(),
            // 恢复路径派生：record_id 非空 ⇒ 强响应。
            None => self.session_id.as_str(),
        };
        if thread.is_empty() || self.record_id.is_empty() || self.session_id.is_empty() {
            return None;
        }
        Some((
            "codex-response",
            format!("codex|1|{thread}|{}", self.record_id),
        ))
    }
}

/// 四类 token 的累计计数，聚合与费用计算共用。
/// N18：附带缓存写的 5m/1h 分项与可信标志——分项是 `cache_write` 的内部
/// 构成，任何累计口径都不得把分项再加进 `cache_write`/总量。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TokenCounts {
    pub input: u64,
    pub output: u64,
    pub cache_write: u64,
    pub cache_read: u64,
    /// N18：5 分钟缓存写分项（`cache_write_split_known == false` 时恒 0）。
    pub cache_write_5m: u64,
    /// N18：1 小时缓存写分项（同上）。
    pub cache_write_1h: u64,
    /// N18：分项可信（逐事件 AND 语义：聚合中混入任一未知分档事件即整体未知）。
    pub cache_write_split_known: bool,
}

impl TokenCounts {
    pub fn from_event(e: &UsageEvent) -> Self {
        Self {
            input: e.input_tokens,
            output: e.output_tokens,
            cache_write: e.cache_write_tokens,
            cache_read: e.cache_read_tokens,
            cache_write_5m: e.cache_write_5m,
            cache_write_1h: e.cache_write_1h,
            cache_write_split_known: e.cache_write_split_known,
        }
    }

    /// Task 1（分段计价）：prompt token 度量 = 非缓存输入 + 缓存写 + 缓存读
    /// （输出不参与上下文档位选择——不变量 2）。N18：写总量已含两档分项，
    /// prompt 基数不再另加分项（不得重复计入）。
    pub fn prompt_tokens(&self) -> u64 {
        self.input + self.cache_write + self.cache_read
    }

    /// 是否尚未累计任何 token（全零）。用于分档可信标志的「首条定值、
    /// 其后 AND」：只有真正的空累计才允许被下一条的已知性覆盖。
    fn is_empty_counts(&self) -> bool {
        self.input == 0 && self.output == 0 && self.cache_write == 0 && self.cache_read == 0
    }

    pub fn add_event(&mut self, e: &UsageEvent) {
        let was_empty = self.is_empty_counts();
        self.input += e.input_tokens;
        self.output += e.output_tokens;
        self.cache_write += e.cache_write_tokens;
        self.cache_read += e.cache_read_tokens;
        self.cache_write_5m += e.cache_write_5m;
        self.cache_write_1h += e.cache_write_1h;
        // 分档可信度是逐事件的属性：总量口径上只要有一个未知分档事件，
        // 聚合就整体退回「分项未知」（不假装聚合结果仍可分档计价）。
        // 首条事件决定初值（空累计必须保持「未知」，不得因默认 false 把
        // 全可信累加成未知）；其后逐条 AND。判空必须看全部四桶——零缓存写
        // 但其他 token 非零的未知事件同样会污染可信标志，且顺序无关。
        if was_empty {
            self.cache_write_split_known = e.cache_write_split_known;
        } else {
            self.cache_write_split_known &= e.cache_write_split_known;
        }
    }

    pub fn add(&mut self, other: &TokenCounts) {
        let was_empty = self.is_empty_counts();
        self.input += other.input;
        self.output += other.output;
        self.cache_write += other.cache_write;
        self.cache_read += other.cache_read;
        self.cache_write_5m += other.cache_write_5m;
        self.cache_write_1h += other.cache_write_1h;
        if was_empty {
            self.cache_write_split_known = other.cache_write_split_known;
        } else {
            self.cache_write_split_known &= other.cache_write_split_known;
        }
    }

    pub fn total(&self) -> u64 {
        self.input + self.output + self.cache_write + self.cache_read
    }

    /// SF08：受检累加——任一桶溢出返回 None（聚合层据此整体报错，
    /// 不回绕、不饱和、不做部分提交）。
    /// N18：分档可信度按「首条定值、其后 AND」与 `add` 一致——空侧
    /// （全零）不把全可信结果拖成未知。判空看全部四桶：零缓存写但其他
    /// token 非零的未知侧同样会污染可信标志。
    pub fn checked_add(&self, other: &TokenCounts) -> Option<TokenCounts> {
        let cache_write = self.cache_write.checked_add(other.cache_write)?;
        let split_known = if self.is_empty_counts() {
            other.cache_write_split_known
        } else if other.is_empty_counts() {
            self.cache_write_split_known
        } else {
            self.cache_write_split_known && other.cache_write_split_known
        };
        Some(TokenCounts {
            input: self.input.checked_add(other.input)?,
            output: self.output.checked_add(other.output)?,
            cache_write,
            cache_read: self.cache_read.checked_add(other.cache_read)?,
            cache_write_5m: self.cache_write_5m.checked_add(other.cache_write_5m)?,
            cache_write_1h: self.cache_write_1h.checked_add(other.cache_write_1h)?,
            cache_write_split_known: split_known,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(input: u64, output: u64, cw: u64, cr: u64) -> UsageEvent {
        UsageEvent {
            ts: "2026-07-17T08:00:00.000Z".parse().unwrap(),
            agent: AgentKind::ClaudeCode,
            record_id: String::new(),
            claude_copy: None,
            codex_response: None,
            codex_legacy_origin: None,
            line: 0,
            source_path: String::new(),
            model: "m".into(),
            session_id: "s".into(),
            project: "p".into(),
            session_initial_cwd: None,
            event_cwd: None,
            input_tokens: input,
            output_tokens: output,
            cache_write_tokens: cw,
            cache_read_tokens: cr,
            cache_write_5m: 0,
            cache_write_1h: 0,
            cache_write_split_known: false,
        }
    }

    #[test]
    fn test_usage_event_total_tokens() {
        let e = event(100, 20, 3, 7);
        assert_eq!(e.total_tokens(), 130);
    }

    #[test]
    fn test_token_counts_accumulate() {
        let mut c = TokenCounts::default();
        c.add_event(&event(1, 2, 3, 4));
        c.add_event(&event(10, 20, 30, 40));
        assert_eq!(c.input, 11);
        assert_eq!(c.output, 22);
        assert_eq!(c.cache_write, 33);
        assert_eq!(c.cache_read, 44);
        assert_eq!(c.total(), 110);
    }
}
