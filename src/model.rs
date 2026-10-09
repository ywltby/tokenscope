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
}

impl AgentKind {
    pub fn as_str(self) -> &'static str {
        match self {
            AgentKind::ClaudeCode => "claude-code",
            AgentKind::Codex => "codex",
        }
    }
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
    /// agent 原生日志标识（Claude 的 message.id；Codex 无此标识记空串），
    /// 供全局去重使用。
    pub record_id: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_write_tokens: u64,
    pub cache_read_tokens: u64,
}

impl UsageEvent {
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens + self.cache_write_tokens + self.cache_read_tokens
    }

    /// SF08：单事件桶边界校验（source→model 公共边界，两适配器与缓存
    /// 恢复经过同一入口）——四桶总数与 prompt 计价基数必须可表示。
    /// 异常事件由调用方计 bad_lines 并跳过，不做回绕/饱和。
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
        Ok(())
    }
}

/// 四类 token 的累计计数，聚合与费用计算共用。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TokenCounts {
    pub input: u64,
    pub output: u64,
    pub cache_write: u64,
    pub cache_read: u64,
}

impl TokenCounts {
    pub fn from_event(e: &UsageEvent) -> Self {
        Self {
            input: e.input_tokens,
            output: e.output_tokens,
            cache_write: e.cache_write_tokens,
            cache_read: e.cache_read_tokens,
        }
    }

    /// Task 1（分段计价）：prompt token 度量 = 非缓存输入 + 缓存写 + 缓存读
    /// （输出不参与上下文档位选择——不变量 2）。
    pub fn prompt_tokens(&self) -> u64 {
        self.input + self.cache_write + self.cache_read
    }

    pub fn add_event(&mut self, e: &UsageEvent) {
        self.input += e.input_tokens;
        self.output += e.output_tokens;
        self.cache_write += e.cache_write_tokens;
        self.cache_read += e.cache_read_tokens;
    }

    pub fn add(&mut self, other: &TokenCounts) {
        self.input += other.input;
        self.output += other.output;
        self.cache_write += other.cache_write;
        self.cache_read += other.cache_read;
    }

    pub fn total(&self) -> u64 {
        self.input + self.output + self.cache_write + self.cache_read
    }

    /// SF08：受检累加——任一桶溢出返回 None（聚合层据此整体报错，
    /// 不回绕、不饱和、不做部分提交）。
    pub fn checked_add(&self, other: &TokenCounts) -> Option<TokenCounts> {
        Some(TokenCounts {
            input: self.input.checked_add(other.input)?,
            output: self.output.checked_add(other.output)?,
            cache_write: self.cache_write.checked_add(other.cache_write)?,
            cache_read: self.cache_read.checked_add(other.cache_read)?,
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
            model: "m".into(),
            session_id: "s".into(),
            project: "p".into(),
            session_initial_cwd: None,
            event_cwd: None,
            input_tokens: input,
            output_tokens: output,
            cache_write_tokens: cw,
            cache_read_tokens: cr,
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
