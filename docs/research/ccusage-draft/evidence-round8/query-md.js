const fs = require('fs');
const md = JSON.parse(fs.readFileSync(process.argv[2], 'utf8'));
const keys = Object.keys(md);
console.log('models-dev 快照键数:', keys.length);
const probes = ['claude-sonnet-4-20250514', 'claude-opus-4-6', 'claude-opus-5', 'claude-3-5-haiku-20241022', 'gpt-5.4', 'gpt-5.3-codex', 'gpt-5.6-sol'];
for (const p of probes) console.log(`  ${p} => ${keys.includes(p) ? '在' : '不在'}`);
const claudeLike = keys.filter((k) => k.toLowerCase().includes('claude'));
console.log('含 claude 的键数:', claudeLike.length);
console.log('样例:', claudeLike.slice(0, 15).join(', '));

const mdKeys = new Set(keys);
const embedded = JSON.parse(fs.readFileSync(process.argv[3], 'utf8'));
const onlyInLitellm = Object.keys(embedded).filter((k) => !mdKeys.has(k));
const both = Object.keys(embedded).filter((k) => mdKeys.has(k));
console.log(`内嵌LiteLLM键 ${Object.keys(embedded).length} | 与 models.dev 重叠 ${both.length} | 仅 LiteLLM ${onlyInLitellm.length}`);

// claude-* 家族在两个源里的完整对照
const claudeLitellm = Object.keys(embedded).filter((k) => k.startsWith('claude-'));
console.log('\nLiteLLM 内嵌的 claude-* 键（全部 ' + claudeLitellm.length + ' 个）:');
console.log(claudeLitellm.join('\n'));
