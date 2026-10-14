const fs = require('fs');
const api = JSON.parse(fs.readFileSync(process.argv[2], 'utf8'));
const embedded = JSON.parse(fs.readFileSync(process.argv[3], 'utf8'));

const providers = Object.keys(api);
let modelCount = 0;
const onlineIds = new Map(); // modelId -> [providers]
for (const p of providers) {
  const prov = api[p];
  if (!prov || !prov.models) continue;
  for (const m of Object.keys(prov.models)) {
    modelCount++;
    if (!onlineIds.has(m)) onlineIds.set(m, []);
    onlineIds.get(m).push(prov.id || p);
  }
}
console.log(`在线目录 provider 数: ${providers.length}`);
console.log(`在线目录模型条目数（含重复）: ${modelCount}`);
console.log(`在线目录去重 model id 数: ${onlineIds.size}`);

const embKeys = Object.keys(embedded);
console.log(`\n内嵌快照键数: ${embKeys.length}`);
const inOnline = embKeys.filter((k) => onlineIds.has(k));
console.log(`内嵌键仍在在线目录: ${inOnline.length}`);
const gone = embKeys.filter((k) => !onlineIds.has(k));
console.log(`内嵌键已不在在线目录: ${gone.length}`);
console.log('已消失样例:', gone.slice(0, 15).join(', '));

// 内嵌快照与新快照的共同键的形态
const probes = ['claude-opus-4-6', 'claude-opus-5', 'gpt-5.4', 'claude-sonnet-4-20250514'];
console.log('\n--- 探针（在线目录）---');
for (const p of probes) console.log(`  ${p}: ${onlineIds.has(p) ? '在（provider: ' + onlineIds.get(p).join(',') + '）' : '不在'}`);

// provider 名单前 30
console.log('\n--- 在线 provider（前 30）---');
console.log(providers.slice(0, 30).join(', '));
