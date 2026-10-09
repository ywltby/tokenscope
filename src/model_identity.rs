//! MP01：模型身份——等价键与结构边界。
//!
//! 规则（docs/plans/active/2026-10-10-model-pricing-name-equivalence.md §2）：
//! - **先解析结构**（`/` 命名空间、`:` 变体），再对各片段做「删除 ASCII
//!   `-` `.` `_` + 转小写」；`/` 与 `:` 保留结构语义，渠道名与显示名不被压缩；
//! - 基名压缩后为空（`-._`、`provider/---`）不是通配价格——调用方按未收录处理；
//! - 记录基名里**分隔符位置**对应的压缩键偏移，供前缀回退只在词元边界截断：
//!   `opus55-20261010` 可回退到 `opus55`，`opus550` 不得命中 `opus55`，
//!   `gpt-50` 不得命中 `gpt5`；
//! - 只对 ASCII 符号与 ASCII 大小写生效，中文等多字节字符原样保留。

/// 等价片段：删除 ASCII `-` / `.` / `_` 并转 ASCII 小写（逐字符，UTF-8 安全）。
pub fn equivalence_fragment(s: &str) -> String {
    s.chars()
        .filter(|c| !is_separator(*c))
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

fn is_separator(c: char) -> bool {
    matches!(c, '-' | '.' | '_')
}

/// 解析后的模型标识：原文、结构、等价键与边界偏移。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelIdentity {
    raw: String,
    namespace: Option<String>,
    leaf: String,
    base: String,
    variant: Option<String>,
    leaf_key: String,
    base_key: String,
    /// 基名等价键中允许截断的**字符长度**（升序、去重；不含 0 与全长）。
    boundaries: Vec<usize>,
}

impl ModelIdentity {
    /// 解析模型标识（trim → 拆 `/` 与 `:` → 各片段压缩）。
    pub fn parse(raw: &str) -> Self {
        let trimmed = raw.trim();
        let (namespace, leaf) = match trimmed.rfind('/') {
            Some(i) => (Some(trimmed[..i].to_string()), &trimmed[i + 1..]),
            None => (None, trimmed),
        };
        let (base, variant) = match leaf.find(':') {
            Some(i) => (&leaf[..i], Some(leaf[i + 1..].to_string())),
            None => (leaf, None),
        };
        let base_key = equivalence_fragment(base);
        let boundaries = separator_boundaries(base);
        let variant_key = variant.as_deref().map(equivalence_fragment);
        // 变体结构（含"空变体"）必须保留在末段键里：`opus55:` 不得等同于 `opus55`。
        let leaf_key = match &variant_key {
            Some(v) => format!("{base_key}:{v}"),
            None => base_key.clone(),
        };
        Self {
            raw: trimmed.to_string(),
            namespace,
            leaf: leaf.to_string(),
            base: base.to_string(),
            variant,
            leaf_key,
            base_key,
            boundaries,
        }
    }

    /// trim 后的原始标识。
    pub fn raw(&self) -> &str {
        &self.raw
    }

    /// 最后一个 `/` 之前的渠道/命名空间原文。
    pub fn namespace(&self) -> Option<&str> {
        self.namespace.as_deref()
    }

    /// 末段原文（`/` 之后）。
    pub fn leaf(&self) -> &str {
        &self.leaf
    }

    /// 基名原文（末段中 `:` 之前）。
    pub fn base(&self) -> &str {
        &self.base
    }

    /// 变体原文（`:` 之后；Some("") = 显式空变体）。
    pub fn variant(&self) -> Option<&str> {
        self.variant.as_deref()
    }

    /// 末段等价键（含 `:` 变体结构）——价格匹配键。
    pub fn leaf_key(&self) -> &str {
        &self.leaf_key
    }

    /// 基名等价键。
    pub fn base_key(&self) -> &str {
        &self.base_key
    }

    /// 变体等价键（`Some("")` = 显式空变体）。
    pub fn variant_key(&self) -> Option<String> {
        self.variant.as_deref().map(equivalence_fragment)
    }

    /// 基名压缩后为空：该标识不能成为通配价格键。
    pub fn is_empty_key(&self) -> bool {
        self.base_key.is_empty()
    }

    /// 允许截断的基名等价键长度（升序、去重）。
    pub fn boundaries(&self) -> &[usize] {
        &self.boundaries
    }

    /// 基名等价键的前 `len` 个字符（`len` 必须来自 [`boundaries`](Self::boundaries)）。
    pub fn base_prefix(&self, len: usize) -> String {
        self.base_key.chars().take(len).collect()
    }

    /// 整标识的等价键：保留 `/` 与 `:` 结构，其余字符压缩（分组身份用）。
    pub fn identity_key(&self) -> String {
        let mut out = String::with_capacity(self.raw.len());
        for c in self.raw.chars() {
            if matches!(c, '/' | ':') {
                out.push(c);
            } else if is_separator(c) {
                continue;
            } else {
                out.push(c.to_ascii_lowercase());
            }
        }
        out
    }
}

/// 基名里分隔符位置对应的压缩键偏移（字符长度）：
/// 只保留 0 < offset < 全长 的值并去重（首尾符号不产生可用边界）。
fn separator_boundaries(base: &str) -> Vec<usize> {
    let total = base.chars().filter(|c| !is_separator(*c)).count();
    let mut out: Vec<usize> = Vec::new();
    let mut effective = 0usize;
    for c in base.chars() {
        if is_separator(c) {
            if effective > 0 && effective < total && out.last() != Some(&effective) {
                out.push(effective);
            }
        } else {
            effective += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_equivalence_key_examples() {
        // 三种拼写（含混合大小写）压缩为同一等价键。
        for s in [
            "claude-opus-5-5",
            "claude-opus-5.5",
            "Claude_Opus_5_5",
            "CLAUDE.OPUS.5.5",
            "claudeopus55",
        ] {
            let id = ModelIdentity::parse(s);
            assert_eq!(id.base_key(), "claudeopus55", "{s}");
            assert_eq!(id.leaf_key(), "claudeopus55", "{s}");
        }
        assert_eq!(ModelIdentity::parse("opus55").base_key(), "opus55");
        assert_eq!(ModelIdentity::parse("opus5.5").base_key(), "opus55");
        assert_eq!(ModelIdentity::parse("Opus5_5").base_key(), "opus55");
        // 空白按 trim 处理，内部空格不新增等价规则。
        assert_eq!(ModelIdentity::parse("  Opus 5.5  ").base_key(), "opus 55");
    }

    #[test]
    fn model_equivalence_preserves_structure() {
        let id = ModelIdentity::parse("nano-gpt/qwen/qwen3.8-27b:thinking");
        assert_eq!(id.namespace(), Some("nano-gpt/qwen"));
        assert_eq!(id.leaf(), "qwen3.8-27b:thinking");
        assert_eq!(id.base(), "qwen3.8-27b");
        assert_eq!(id.base_key(), "qwen3827b");
        assert_eq!(id.variant(), Some("thinking"));
        assert_eq!(id.variant_key().as_deref(), Some("thinking"));
        assert_eq!(id.leaf_key(), "qwen3827b:thinking");
        // 分组身份保留 `/` 与 `:` 结构，渠道大小写折叠、符号压缩。
        assert_eq!(id.identity_key(), "nanogpt/qwen/qwen3827b:thinking");
        // 中文只受 ASCII 符号与大小写规则影响。
        assert_eq!(ModelIdentity::parse("模型-α.1").base_key(), "模型α1");
        // 变体大小写折叠，但变体语义（free/thinking）不合并。
        assert_eq!(
            ModelIdentity::parse("Opus55:FREE").leaf_key(),
            "opus55:free"
        );
        assert_ne!(
            ModelIdentity::parse("Opus55:FREE").leaf_key(),
            ModelIdentity::parse("Opus55:thinking").leaf_key()
        );
    }

    #[test]
    fn model_equivalence_rejects_empty_base() {
        for s in ["-._", "---", "_", ".", "provider/---", "provider/-._"] {
            assert!(ModelIdentity::parse(s).is_empty_key(), "{s}");
        }
        // 尾部符号不构成空键。
        assert!(!ModelIdentity::parse("a-").is_empty_key());
        assert_eq!(ModelIdentity::parse("a-").base_key(), "a");
        // 显式空变体不悄悄变成基础模型。
        let id = ModelIdentity::parse("opus55:");
        assert_eq!(id.variant(), Some(""));
        assert_eq!(id.leaf_key(), "opus55:");
        assert_ne!(id.leaf_key(), "opus55");
    }

    #[test]
    fn model_equivalence_boundary_offsets() {
        let id = ModelIdentity::parse("opus55-20261010");
        assert_eq!(id.base_key(), "opus5520261010");
        assert_eq!(id.boundaries(), [6], "只有分隔符处可截断");
        assert_eq!(id.base_prefix(6), "opus55");

        assert_eq!(ModelIdentity::parse("gpt-50").boundaries(), [3]);
        assert_eq!(ModelIdentity::parse("gpt-50").base_prefix(3), "gpt");
        // 无分隔符：不得猜测后缀边界。
        assert!(ModelIdentity::parse("opus550").boundaries().is_empty());
        assert!(ModelIdentity::parse("gpt50").boundaries().is_empty());

        // 连续分隔符去重；首尾符号不产生 0/全长边界。
        let id = ModelIdentity::parse("-opus--5_5-");
        assert_eq!(id.base_key(), "opus55");
        assert_eq!(id.boundaries(), [4, 5]);

        // 多字节字符：边界按字符计数，切片安全。
        let id = ModelIdentity::parse("项目.版本-1");
        assert_eq!(id.base_key(), "项目版本1");
        assert_eq!(id.boundaries(), [2, 4]);
        assert_eq!(id.base_prefix(4), "项目版本");
    }
}
