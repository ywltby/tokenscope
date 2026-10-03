//! 归一化用量事件与 token 计数：source 层输出 `UsageEvent` 后，下游只认这里的类型。

use jiff::Timestamp;
use serde::Serialize;

/// 已接入的 agent 种类；新增适配器时在此扩枚举。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentKind {
    ClaudeCode,
}

impl AgentKind {
    pub fn as_str(self) -> &'static str {
        match self {
            AgentKind::ClaudeCode => "claude-code",
        }
    }
}

/// 一条 assistant 消息的用量（去重后），agent 无关的归一形态。
#[derive(Debug, Clone)]
pub struct UsageEvent {
    pub ts: Timestamp,
    pub agent: AgentKind,
    pub model: String,
    pub session_id: String,
    pub project: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_write_tokens: u64,
    pub cache_read_tokens: u64,
}

impl UsageEvent {
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens + self.cache_write_tokens + self.cache_read_tokens
    }
}

/// 四类 token 的累计计数，聚合与费用计算共用。
#[derive(Debug, Default, Clone, Copy, Serialize)]
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(input: u64, output: u64, cw: u64, cr: u64) -> UsageEvent {
        UsageEvent {
            ts: "2026-07-17T08:00:00.000Z".parse().unwrap(),
            agent: AgentKind::ClaudeCode,
            model: "m".into(),
            session_id: "s".into(),
            project: "p".into(),
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
