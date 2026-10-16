//! 持久格式指纹（UP03）：把散落在各模块的"落盘格式版本"常量汇成一束，
//! 供生成 `build-metadata.json` 的构建作业写进 `persistent_format` 字段。
//!
//! 这里**只读既有常量**，不复制任何数字：谁改了版本号，指纹与示例输出自动
//! 跟着变，不存在"代码改了、登记表没跟上"的第二份真相。构建作业运行
//! `cargo run --locked --release -p tokenscope --example print_persistent_formats`
//! 并把标准输出原样写进 `build-metadata.json`（schema 2 的 `persistent_format`）。

use serde::Serialize;

use crate::cache::SCHEMA_VERSION as CACHE_SCHEMA;
use crate::history::{HISTORY_SCHEMA_VERSION, IDENTITY_REVISION, PARSER_REVISION};
use crate::pricing::INDEX_VERSION;
use crate::settings::SETTINGS_FORMAT_REVISION;

/// 六项持久格式指纹。字段值**全为字符串**（整数常量经 `to_string()`），
/// 键名与顺序即上游契约：登记表 `persistent_format` 逐项对照，见计划 §4.2。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PersistentFormat {
    /// 解析结果缓存结构版本（`~/.tokenscope/cache.db`，改动即整体失效重解析）。
    pub cache_schema: String,
    /// 统一用量历史库结构版本（`~/.tokenscope/history.db` 的 `PRAGMA user_version`）。
    pub history_schema: String,
    /// 事件解析规则版本（历史库 `history_meta.parser_revision`）。
    pub parser_revision: String,
    /// 模型身份等价键规则版本（历史库 `history_meta.identity_revision`）。
    pub identity_revision: String,
    /// 价格索引版本（`~/.tokenscope/pricing-index.json`）。
    pub pricing_index: String,
    /// 设置格式修订号（[`SETTINGS_FORMAT_REVISION`]，决定 `settings.toml` 的解释语义）。
    pub settings_format: String,
}

impl PersistentFormat {
    /// 一行紧凑 JSON（无多余空格；键顺序 = 字段声明顺序）。
    ///
    /// 六个字段都是字符串，序列化不会失败；真失败说明结构被改成了不可序列化
    /// 的形态，属于编码错误，直接 panic，绝不吞掉后输出半截指纹。
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("持久格式指纹只有字符串字段，序列化不会失败")
    }
}

/// 取当前指纹：每一项都直接读真实常量，不做任何转换或复制。
pub fn persistent_format() -> PersistentFormat {
    PersistentFormat {
        cache_schema: CACHE_SCHEMA.to_string(),
        history_schema: HISTORY_SCHEMA_VERSION.to_string(),
        parser_revision: PARSER_REVISION.to_string(),
        identity_revision: IDENTITY_REVISION.to_string(),
        pricing_index: INDEX_VERSION.to_string(),
        settings_format: SETTINGS_FORMAT_REVISION.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 六项必须就是各模块常量本身——任何一处改版本号都会在这里立刻变红，
    /// 避免"指纹还停在旧值"的静默漂移。
    #[test]
    fn persistent_format_matches_real_constants() {
        let f = persistent_format();
        assert_eq!(f.cache_schema, crate::cache::SCHEMA_VERSION);
        assert_eq!(f.history_schema, HISTORY_SCHEMA_VERSION.to_string());
        assert_eq!(f.parser_revision, PARSER_REVISION.to_string());
        assert_eq!(f.identity_revision, IDENTITY_REVISION.to_string());
        assert_eq!(f.pricing_index, INDEX_VERSION.to_string());
        assert_eq!(f.settings_format, SETTINGS_FORMAT_REVISION.to_string());
    }

    /// 键名与顺序是外部契约（`build-metadata.json` 与登记表逐项对照）：
    /// 期望串由真实常量拼出，逐字节比对，改名、重排、漏项都会失败。
    #[test]
    fn persistent_format_json_keys_and_order_are_fixed() {
        let expected = format!(
            r#"{{"cache_schema":"{}","history_schema":"{}","parser_revision":"{}","identity_revision":"{}","pricing_index":"{}","settings_format":"{}"}}"#,
            crate::cache::SCHEMA_VERSION,
            HISTORY_SCHEMA_VERSION,
            PARSER_REVISION,
            IDENTITY_REVISION,
            INDEX_VERSION,
            SETTINGS_FORMAT_REVISION,
        );
        assert_eq!(persistent_format().to_json(), expected);
    }
}
