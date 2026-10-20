const fs = require('fs');
const full = JSON.parse(fs.readFileSync(process.argv[2], 'utf8'));
const probes = ['claude-sonnet-4-20250514','claude-3-5-haiku-20241022','claude-3-5-sonnet-20241022','claude-opus-4-1-20250805','claude-sonnet-4-5-20250929'];
console.log('=== 完整 LiteLLM（未裁剪）中的旧模型 ===');
for (const p of probes) console.log(`  ${p} => ${full[p] ? '在' : '不在'}`);
const claudeKeys = Object.keys(full).filter(k => k.startsWith('claude-'));
console.log('完整表含 claude-* 键数:', claudeKeys.length);