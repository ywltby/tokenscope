// 重放 ccusage-core/build.rs 的 compact_pricing_json + is_embedded_model 逻辑
// 用法: node replay-embed.js <litellm-json> <output-json>
const fs = require('fs');

const src = process.argv[2];
const out = process.argv[3];
const raw = JSON.parse(fs.readFileSync(src, 'utf8'));

// build.rs:203-221
function isEmbeddedModel(m) {
  return (
    m.startsWith('claude-') ||
    m.startsWith('anthropic.') ||
    m.startsWith('anthropic/') ||
    m.startsWith('us.anthropic.') ||
    m.startsWith('eu.anthropic.') ||
    m.startsWith('global.anthropic.') ||
    m.startsWith('jp.anthropic.') ||
    m.startsWith('au.anthropic.') ||
    m.startsWith('gpt-') ||
    m === 'o3' ||
    m.startsWith('o3-') ||
    m === 'o4-mini' ||
    m.startsWith('o4-mini-') ||
    m.startsWith('openai/') ||
    m.startsWith('azure/') ||
    m.startsWith('zai/') ||
    m.startsWith('openrouter/openai/')
  );
}

// build.rs:168-195
const FIELD_MAP = [
  ['input_cost_per_token', 'i'],
  ['output_cost_per_token', 'o'],
  ['cache_creation_input_token_cost', 'cc'],
  ['cache_read_input_token_cost', 'cr'],
  ['input_cost_per_token_above_200k_tokens', 'ia'],
  ['output_cost_per_token_above_200k_tokens', 'oa'],
  ['cache_creation_input_token_cost_above_200k_tokens', 'cca'],
  ['cache_read_input_token_cost_above_200k_tokens', 'cra'],
  ['max_input_tokens', 'ctx'],
];

const keep = {};
for (const [model, pricing] of Object.entries(raw)) {
  if (!isEmbeddedModel(model)) continue;
  if (typeof pricing !== 'object' || pricing === null || Array.isArray(pricing)) continue;
  const fields = {};
  for (const [source, target] of FIELD_MAP) {
    const value = pricing[source];
    if (value === undefined || value === null) continue;
    fields[target] = value;
  }
  for (const tier of ['fast', 'flex']) {
    const value = pricing.provider_specific_entry && pricing.provider_specific_entry[tier];
    if (value === undefined || value === null) continue;
    fields[tier] = value;
  }
  // build.rs:196-198
  if (fields.i !== undefined && fields.o !== undefined) keep[model] = fields;
}

const keys = Object.keys(keep);
console.log(`原始条目: ${Object.keys(raw).length}`);
console.log(`内嵌键数: ${keys.length}`);
console.log(`被白名单筛掉: ${Object.keys(raw).length - keys.length}`);

const probes = [
  'claude-sonnet-4-20250514',
  'claude-opus-4-6',
  'claude-opus-4-6-fast',
  'claude-opus-5',
  'claude-opus-5-fast',
  'claude-3-5-haiku-20241022',
  'gpt-5',
  'gpt-5.4',
  'gpt-5.3-codex',
  'gpt-5.6-sol',
  'o3',
  'o4-mini',
];
console.log('\n--- 探针 ---');
for (const probe of probes) {
  console.log(`${probe} => ${keep[probe] ? JSON.stringify(keep[probe]) : '不在内嵌表'}`);
}

// 前缀分布
const buckets = {};
const bucket = (k) => {
  if (k.startsWith('claude-')) return 'claude-';
  if (k.startsWith('gpt-')) return 'gpt-';
  if (k.startsWith('openai/')) return 'openai/';
  if (k.startsWith('azure/')) return 'azure/';
  if (k.startsWith('zai/')) return 'zai/';
  if (k.startsWith('openrouter/openai/')) return 'openrouter/openai/';
  if (k === 'o3' || k.startsWith('o3-') || k === 'o4-mini' || k.startsWith('o4-mini-')) return 'o3/o4-mini';
  return '其它-' + k.split(/[./-]/)[0];
};
for (const k of keys) buckets[bucket(k)] = (buckets[bucket(k)] || 0) + 1;
console.log('\n--- 前缀分布 ---');
console.log(JSON.stringify(buckets, null, 2));

// 带 fast/flex 倍率的键
const withFast = keys.filter((k) => keep[k].fast !== undefined);
const withFlex = keys.filter((k) => keep[k].flex !== undefined);
console.log(`\n带 fast 倍率: ${withFast.length} 个 -> ${withFast.slice(0, 8).join(', ')}`);
console.log(`带 flex 倍率: ${withFlex.length} 个 -> ${withFlex.slice(0, 8).join(', ')}`);

fs.writeFileSync(out, JSON.stringify(keep));
console.log(`\n已写出: ${out}`);
