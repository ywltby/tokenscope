// 重放 pricing.rs 的匹配算法（pricing_key_matches / contains_pricing_key / max_by）
// 输入：内嵌裁剪表（embedded-keys.json，来自真实 LiteLLM pin 版）
// 用法: node replay-match.js <embedded-keys.json> <probe...>
const fs = require('fs');
const MODEL_DATE_SUFFIX_DIGITS = 8;

const table = JSON.parse(fs.readFileSync(process.argv[2], 'utf8'));
const entries = new Map(Object.entries(table));

function isPricingKeyBoundary(byte) {
  // !byte.is_ascii_alphanumeric()
  const ch = String.fromCharCode(byte);
  return !/[0-9A-Za-z]/.test(ch);
}

function suffixStartsWithNumericModelVersion(key, suffix) {
  if (!/^[0-9]$/.test(key[key.length - 1] || '')) return false;
  const first = suffix[0];
  if (first !== '-' && first !== '.') return false;
  const rest = suffix.slice(1);
  let digitLen = 0;
  while (digitLen < rest.length && /[0-9]/.test(rest[digitLen])) digitLen++;
  if (digitLen === 0) return false;
  const afterDigits = digitLen < rest.length ? rest[digitLen] : null;
  return !(digitLen === MODEL_DATE_SUFFIX_DIGITS && (afterDigits === null || isPricingKeyBoundary(afterDigits.charCodeAt(0))));
}

function suffixAllowsPricingKeyMatch(key, suffix) {
  if (suffix.length === 0) return true;
  const separator = suffix.charCodeAt(0);
  if (!isPricingKeyBoundary(separator)) return false;
  return !suffixStartsWithNumericModelVersion(key, suffix);
}

function containsPricingKey(value, key) {
  let from = 0;
  for (;;) {
    const index = value.indexOf(key, from);
    if (index === -1) return false;
    const before = index > 0 ? value.charCodeAt(index - 1) : null;
    const suffix = value.slice(index + key.length);
    if ((before === null || isPricingKeyBoundary(before)) && suffixAllowsPricingKeyMatch(key, suffix)) return true;
    from = index + 1;
  }
}

function normalizedPricingKey(value) {
  if (value.includes('.') || value.includes('@')) return value.replace(/[.@]/g, '-');
  return value;
}

function pricingKeyMatches(candidate, model) {
  if (containsPricingKey(model, candidate) || containsPricingKey(candidate, model)) return true;
  const nc = normalizedPricingKey(candidate);
  const nm = normalizedPricingKey(model);
  return containsPricingKey(nm, nc) || containsPricingKey(nc, nm);
}

// find_entry_with_fallback 的模糊段：全表扫描 + max_by(len, then right.cmp(left))
// Rust: max_by(|l, r| l.len().cmp(&r.len()).then_with(|| r.cmp(l)))
// → 最长键优先；长度相同时「字典序较小者」胜（因为 left 在 r.cmp(l) 为 Greater 时更大）
function fuzzyLookup(model) {
  const normalizedModel = normalizedPricingKey(model);
  const matches = [];
  for (const candidate of entries.keys()) {
    if (pricingKeyMatches(candidate, model, normalizedModel)) matches.push(candidate);
  }
  if (matches.length === 0) return null;
  matches.sort((a, b) => {
    if (a.length !== b.length) return b.length - a.length;
    return a < b ? -1 : a > b ? 1 : 0;
  });
  return matches[0];
}

const probes = process.argv.slice(3);
for (const probe of probes) {
  const exact = entries.has(probe) ? '精确命中' : '无精确键';
  const fuzzy = fuzzyLookup(probe);
  console.log(`${probe}\n  ${exact}；模糊命中: ${fuzzy || '（无）'}${fuzzy ? ` [len=${fuzzy.length}]` : ''}`);
  if (fuzzy && fuzzy !== probe) console.log(`  命中价: ${JSON.stringify(entries.get(fuzzy))}`);
}
