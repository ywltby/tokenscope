const fs = require('fs');
const full = JSON.parse(fs.readFileSync(process.argv[2], 'utf8'));
const keys = Object.keys(full);

const grep = (needle) => keys.filter((k) => k.includes(needle));
console.log('=== 含 "sonnet-4" 的键 ===');
console.log(grep('sonnet-4').join('\n') || '（无）');
console.log('\n=== 含 "sonnet-3" 的键 ===');
console.log(grep('sonnet-3').join('\n') || '（无）');
console.log('\n=== 含 "opus-4-1" 的键 ===');
console.log(grep('opus-4-1').join('\n') || '（无）');
console.log('\n=== 全部 claude-* 键（23）===');
console.log(keys.filter((k) => k.startsWith('claude-')).join('\n'));
console.log('\n=== 含 "anthropic" 的键数与前 20 ===');
const anth = keys.filter((k) => k.toLowerCase().includes('anthropic'));
console.log('共', anth.length);
console.log(anth.slice(0, 20).join('\n'));
