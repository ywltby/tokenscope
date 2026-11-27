//! 零依赖任意精度无符号整数（类似 Python `int` 的无限长非负计数）。
//!
//! 仅用于**聚合累计总量**（`Group.tokens` / `unknown_tokens` / `RollupCoverage` /
//! `HistoryDb::totals`）；单条请求事件（`UsageEvent`）仍保留 `u64` 与受检校验。
//!
//! # 表示与不变量
//! - `Small(u64)`：所有常规 `<= u64::MAX` 的计数值零堆分配、单指令累加；
//! - `Big(Vec<u64>)`：小端 $10^{18}$ 进制分片（每片恰为 18 位十进制，不含高位前导零，
//!   且数值严格大于 `u64::MAX`）。规范化保证每个非负整数具有唯一内部表示。
//!
//! # JSON 精确安全编码
//! - `<= 2^53 - 1`（JS `Number.MAX_SAFE_INTEGER`）：序列化为 JSON 整数，与既有 IPC/CLI
//!   契约完全兼容；
//! - `> 2^53 - 1`：序列化为十进制 JSON 字符串，防止前端 `JSON.parse()` 按 IEEE-754
//!   `f64` 截断尾数。反序列化同时接受 JSON 非负整数与十进制字符串。

use std::cmp::Ordering;
use std::fmt;
use std::ops::{Add, AddAssign};
use std::str::FromStr;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// 每片基数 $10^{18}$：两片相加加进位最大为 $2\times 10^{18}-1 < \text{u64::MAX}$，
/// 永不溢出 `u64`，且十进制展示与解析直接按 18 位切片完成（$O(n)$、无除法）。
const BASE: u64 = 1_000_000_000_000_000_000;
const BASE_128: u128 = 1_000_000_000_000_000_000;
const LIMB_DIGITS: usize = 18;

/// JS `Number.MAX_SAFE_INTEGER`（$2^{53} - 1 = 9,007,199,254,740,991$）。
/// 超过此值在 JSON 中以十进制字符串传输，前端用原生 `BigInt` 无损解析。
pub const MAX_SAFE_JS_INT: u64 = (1u64 << 53) - 1;

/// 将 `u64` 按与 [`BigUint`] 相同的精确安全规则序列化：
/// `<= MAX_SAFE_JS_INT` 输出 JSON 整数，超过后输出十进制字符串，避免前端 `JSON.parse` 舍入。
pub fn serialize_u64_safe<S: Serializer>(v: &u64, serializer: S) -> Result<S::Ok, S::Error> {
    if *v <= MAX_SAFE_JS_INT {
        serializer.serialize_u64(*v)
    } else {
        serializer.serialize_str(&v.to_string())
    }
}

/// 供 `#[serde(with = "crate::bigint::serde_u64")]` 使用的 `u64` 精确安全编解码模块
/// （同时兼容 JSON 非负整数与十进制字符串反序列化）。
pub mod serde_u64 {
    use serde::{Deserialize, Deserializer, Serializer};

    use super::{BigUint, serialize_u64_safe};

    pub fn serialize<S: Serializer>(v: &u64, serializer: S) -> Result<S::Ok, S::Error> {
        serialize_u64_safe(v, serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
        let big = BigUint::deserialize(deserializer)?;
        big.to_u64()
            .ok_or_else(|| serde::de::Error::custom("u64 字段超出可表示范围"))
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
enum Repr {
    Small(u64),
    Big(Vec<u64>),
}

/// 任意精度非负整数（支持无限长度累计，永不溢出）。
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct BigUint(Repr);

impl BigUint {
    pub const ZERO: Self = Self(Repr::Small(0));

    pub fn is_zero(&self) -> bool {
        matches!(self.0, Repr::Small(0))
    }

    /// 额外堆缓冲字节数（`Small` 零堆分配返回 0，`Big` 返回分片容量字节数，供 RC05 记账）。
    pub fn heap_bytes(&self) -> usize {
        match &self.0 {
            Repr::Small(_) => 0,
            Repr::Big(limbs) => limbs.capacity().saturating_mul(std::mem::size_of::<u64>()),
        }
    }

    pub fn to_u64(&self) -> Option<u64> {
        match self.0 {
            Repr::Small(v) => Some(v),
            Repr::Big(_) => None,
        }
    }

    pub fn to_u128(&self) -> Option<u128> {
        match &self.0 {
            Repr::Small(v) => Some(u128::from(*v)),
            Repr::Big(limbs) => {
                let mut acc = 0u128;
                for &limb in limbs.iter().rev() {
                    acc = acc.checked_mul(BASE_128)?;
                    acc = acc.checked_add(u128::from(limb))?;
                }
                Some(acc)
            }
        }
    }

    /// 转为最接近的 IEEE-754 `f64`（用于图表等浮点场景；超出 `f64::MAX` 返回 `+inf`）。
    pub fn to_f64(&self) -> f64 {
        match &self.0 {
            Repr::Small(v) => *v as f64,
            Repr::Big(_) => self.to_string().parse::<f64>().unwrap_or(f64::INFINITY),
        }
    }

    fn from_limbs(mut limbs: Vec<u64>) -> Self {
        while limbs.last() == Some(&0) {
            limbs.pop();
        }
        match limbs.len() {
            0 => Self::ZERO,
            1 => Self(Repr::Small(limbs[0])),
            2 => {
                let val = u128::from(limbs[1]) * BASE_128 + u128::from(limbs[0]);
                if let Ok(small) = u64::try_from(val) {
                    Self(Repr::Small(small))
                } else {
                    Self(Repr::Big(limbs))
                }
            }
            _ => Self(Repr::Big(limbs)),
        }
    }
}

fn u64_to_limbs(v: u64) -> ([u64; 2], usize) {
    if v == 0 {
        ([0, 0], 0)
    } else if v < BASE {
        ([v, 0], 1)
    } else {
        ([v % BASE, v / BASE], 2)
    }
}

fn add_limbs_in_place(dst: &mut Vec<u64>, rhs: &[u64]) {
    if rhs.is_empty() {
        return;
    }
    if dst.len() < rhs.len() {
        dst.resize(rhs.len(), 0);
    }
    let mut carry = 0u64;
    for (i, &r) in rhs.iter().enumerate() {
        let sum = dst[i] + r + carry;
        if sum >= BASE {
            dst[i] = sum - BASE;
            carry = 1;
        } else {
            dst[i] = sum;
            carry = 0;
        }
    }
    let mut idx = rhs.len();
    while carry > 0 {
        if idx < dst.len() {
            let sum = dst[idx] + carry;
            if sum >= BASE {
                dst[idx] = sum - BASE;
                carry = 1;
            } else {
                dst[idx] = sum;
                carry = 0;
            }
            idx += 1;
        } else {
            dst.push(carry);
            carry = 0;
        }
    }
}

impl Default for BigUint {
    fn default() -> Self {
        Self::ZERO
    }
}

impl From<u64> for BigUint {
    fn from(v: u64) -> Self {
        Self(Repr::Small(v))
    }
}

impl From<usize> for BigUint {
    fn from(v: usize) -> Self {
        Self::from(v as u64)
    }
}

impl From<u128> for BigUint {
    fn from(mut v: u128) -> Self {
        if let Ok(small) = u64::try_from(v) {
            return Self(Repr::Small(small));
        }
        let mut limbs = Vec::with_capacity(3);
        while v > 0 {
            limbs.push((v % BASE_128) as u64);
            v /= BASE_128;
        }
        Self::from_limbs(limbs)
    }
}

impl fmt::Display for BigUint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Repr::Small(v) => write!(f, "{v}"),
            Repr::Big(limbs) => {
                let mut iter = limbs.iter().rev();
                if let Some(&hi) = iter.next() {
                    write!(f, "{hi}")?;
                    for &limb in iter {
                        write!(f, "{limb:018}")?;
                    }
                }
                Ok(())
            }
        }
    }
}

impl fmt::Debug for BigUint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseBigUintError;

impl fmt::Display for ParseBigUintError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "非法的非负十进制整数")
    }
}

impl std::error::Error for ParseBigUintError {}

impl FromStr for BigUint {
    type Err = ParseBigUintError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
            return Err(ParseBigUintError);
        }
        let trimmed = s.trim_start_matches('0');
        if trimmed.is_empty() {
            return Ok(Self::ZERO);
        }
        if let Ok(small) = trimmed.parse::<u64>() {
            return Ok(Self(Repr::Small(small)));
        }
        let mut limbs = Vec::with_capacity(trimmed.len().div_ceil(LIMB_DIGITS));
        let mut end = trimmed.len();
        while end > 0 {
            let start = end.saturating_sub(LIMB_DIGITS);
            let chunk = trimmed[start..end]
                .parse::<u64>()
                .map_err(|_| ParseBigUintError)?;
            limbs.push(chunk);
            end = start;
        }
        Ok(Self::from_limbs(limbs))
    }
}

impl Ord for BigUint {
    fn cmp(&self, other: &Self) -> Ordering {
        match (&self.0, &other.0) {
            (Repr::Small(a), Repr::Small(b)) => a.cmp(b),
            (Repr::Small(_), Repr::Big(_)) => Ordering::Less,
            (Repr::Big(_), Repr::Small(_)) => Ordering::Greater,
            (Repr::Big(a), Repr::Big(b)) => a
                .len()
                .cmp(&b.len())
                .then_with(|| a.iter().rev().cmp(b.iter().rev())),
        }
    }
}

impl PartialOrd for BigUint {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq<u64> for BigUint {
    fn eq(&self, other: &u64) -> bool {
        matches!(self.0, Repr::Small(v) if v == *other)
    }
}

impl PartialEq<BigUint> for u64 {
    fn eq(&self, other: &BigUint) -> bool {
        other == self
    }
}

impl PartialOrd<u64> for BigUint {
    fn partial_cmp(&self, other: &u64) -> Option<Ordering> {
        Some(match self.0 {
            Repr::Small(v) => v.cmp(other),
            Repr::Big(_) => Ordering::Greater,
        })
    }
}

impl PartialEq<i32> for BigUint {
    fn eq(&self, other: &i32) -> bool {
        u64::try_from(*other).is_ok_and(|u| self == &u)
    }
}

impl PartialEq<BigUint> for i32 {
    fn eq(&self, other: &BigUint) -> bool {
        other == self
    }
}

impl PartialOrd<i32> for BigUint {
    fn partial_cmp(&self, other: &i32) -> Option<Ordering> {
        Some(match u64::try_from(*other) {
            Ok(u) => match self.0 {
                Repr::Small(v) => v.cmp(&u),
                Repr::Big(_) => Ordering::Greater,
            },
            Err(_) => Ordering::Greater,
        })
    }
}

impl PartialEq<u128> for BigUint {
    fn eq(&self, other: &u128) -> bool {
        self.to_u128() == Some(*other)
    }
}

impl AddAssign<u64> for BigUint {
    fn add_assign(&mut self, rhs: u64) {
        if rhs == 0 {
            return;
        }
        match &mut self.0 {
            Repr::Small(v) => {
                if let Some(sum) = v.checked_add(rhs) {
                    *v = sum;
                } else {
                    let sum = u128::from(*v) + u128::from(rhs);
                    self.0 = Repr::Big(vec![(sum % BASE_128) as u64, (sum / BASE_128) as u64]);
                }
            }
            Repr::Big(limbs) => {
                let (buf, len) = u64_to_limbs(rhs);
                add_limbs_in_place(limbs, &buf[..len]);
            }
        }
    }
}

impl AddAssign<&BigUint> for BigUint {
    fn add_assign(&mut self, rhs: &BigUint) {
        match &rhs.0 {
            Repr::Small(r) => *self += *r,
            Repr::Big(r_limbs) => match &mut self.0 {
                Repr::Small(v) => {
                    let (buf, len) = u64_to_limbs(*v);
                    let mut out = r_limbs.clone();
                    add_limbs_in_place(&mut out, &buf[..len]);
                    self.0 = Repr::Big(out);
                }
                Repr::Big(l_limbs) => {
                    add_limbs_in_place(l_limbs, r_limbs);
                }
            },
        }
    }
}

impl AddAssign<BigUint> for BigUint {
    fn add_assign(&mut self, rhs: BigUint) {
        *self += &rhs;
    }
}

impl Add<u64> for BigUint {
    type Output = BigUint;

    fn add(mut self, rhs: u64) -> Self::Output {
        self += rhs;
        self
    }
}

impl Add<&BigUint> for &BigUint {
    type Output = BigUint;

    fn add(self, rhs: &BigUint) -> Self::Output {
        let mut out = self.clone();
        out += rhs;
        out
    }
}

impl Add<BigUint> for BigUint {
    type Output = BigUint;

    fn add(mut self, rhs: BigUint) -> Self::Output {
        self += &rhs;
        self
    }
}

impl Add<&BigUint> for BigUint {
    type Output = BigUint;

    fn add(mut self, rhs: &BigUint) -> Self::Output {
        self += rhs;
        self
    }
}

impl std::iter::Sum for BigUint {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        let mut acc = Self::ZERO;
        for item in iter {
            acc += &item;
        }
        acc
    }
}

impl Serialize for BigUint {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            Repr::Small(v) if v <= MAX_SAFE_JS_INT => serializer.serialize_u64(v),
            _ => serializer.serialize_str(&self.to_string()),
        }
    }
}

impl<'de> Deserialize<'de> for BigUint {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct BigUintVisitor;

        impl Visitor<'_> for BigUintVisitor {
            type Value = BigUint;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "非负整数或十进制数字字符串")
            }

            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(BigUint::from(v))
            }

            fn visit_u128<E: de::Error>(self, v: u128) -> Result<Self::Value, E> {
                Ok(BigUint::from(v))
            }

            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
                u64::try_from(v)
                    .map(BigUint::from)
                    .map_err(|_| E::custom("计数不能为负数"))
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                v.parse::<BigUint>().map_err(E::custom)
            }
        }

        deserializer.deserialize_any(BigUintVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_and_u64_overflow_transition_is_exact() {
        let mut n = BigUint::from(u64::MAX - 10);
        assert_eq!(n, u64::MAX - 10);
        n += 10u64;
        assert_eq!(n, u64::MAX);
        assert!(matches!(n.0, Repr::Small(u64::MAX)));

        n += 1u64;
        assert_eq!(n.to_string(), "18446744073709551616");
        assert!(n > u64::MAX);
        assert_eq!(n.to_u64(), None);
        assert_eq!(n.to_u128(), Some(u128::from(u64::MAX) + 1));
    }

    #[test]
    fn arbitrary_length_addition_across_multiple_limbs() {
        let a: BigUint = "99999999999999999999999999999999999999999999999999"
            .parse()
            .unwrap();
        let b: BigUint = "1".parse().unwrap();
        let sum = &a + &b;
        assert_eq!(
            sum.to_string(),
            "100000000000000000000000000000000000000000000000000"
        );
        assert!(sum > a);

        // 连续超过 u128::MAX
        let u128_max = BigUint::from(u128::MAX);
        let doubled = &u128_max + &u128_max;
        assert_eq!(
            doubled.to_string(),
            "680564733841876926926749214863536422910"
        );
        assert_eq!(doubled.to_u128(), None);
    }

    #[test]
    fn parse_strips_leading_zeros_and_rejects_invalid() {
        assert_eq!("0".parse::<BigUint>().unwrap(), 0u64);
        assert_eq!("000000".parse::<BigUint>().unwrap(), 0u64);
        assert_eq!(
            "00018446744073709551615".parse::<BigUint>().unwrap(),
            u64::MAX
        );
        assert!(matches!(
            "00018446744073709551615".parse::<BigUint>().unwrap().0,
            Repr::Small(u64::MAX)
        ));
        let big = "1234567890123456789012345678901234567890"
            .parse::<BigUint>()
            .unwrap();
        assert_eq!(big.to_string(), "1234567890123456789012345678901234567890");
        assert!("".parse::<BigUint>().is_err());
        assert!("-1".parse::<BigUint>().is_err());
        assert!("12a3".parse::<BigUint>().is_err());
    }

    #[test]
    fn json_serialization_switches_to_string_above_js_max_safe_int() {
        let safe = BigUint::from(MAX_SAFE_JS_INT);
        let safe_json = serde_json::to_value(&safe).unwrap();
        assert_eq!(safe_json, serde_json::json!(MAX_SAFE_JS_INT));
        assert_eq!(serde_json::from_value::<BigUint>(safe_json).unwrap(), safe);

        let unsafe_js = BigUint::from(MAX_SAFE_JS_INT + 1);
        let unsafe_json = serde_json::to_value(&unsafe_js).unwrap();
        assert_eq!(unsafe_json, serde_json::json!("9007199254740992"));
        assert_eq!(
            serde_json::from_value::<BigUint>(unsafe_json).unwrap(),
            unsafe_js
        );

        let huge: BigUint = "123456789012345678901234567890".parse().unwrap();
        let huge_json = serde_json::to_value(&huge).unwrap();
        assert_eq!(
            huge_json,
            serde_json::json!("123456789012345678901234567890")
        );
        assert_eq!(serde_json::from_value::<BigUint>(huge_json).unwrap(), huge);
    }

    #[test]
    fn serde_u64_matches_biguint_safe_int_boundary() {
        #[derive(Debug, PartialEq, Serialize, Deserialize)]
        struct Boxed {
            #[serde(with = "serde_u64")]
            n: u64,
        }

        let safe = Boxed { n: MAX_SAFE_JS_INT };
        let safe_json = serde_json::to_value(&safe).unwrap();
        assert_eq!(safe_json["n"], serde_json::json!(MAX_SAFE_JS_INT));
        assert_eq!(serde_json::from_value::<Boxed>(safe_json).unwrap(), safe);

        let unsafe_js = Boxed {
            n: 9_007_199_254_740_993,
        };
        let unsafe_json = serde_json::to_value(&unsafe_js).unwrap();
        assert_eq!(unsafe_json["n"], serde_json::json!("9007199254740993"));
        assert_eq!(
            serde_json::from_value::<Boxed>(unsafe_json).unwrap(),
            unsafe_js
        );
    }
}
