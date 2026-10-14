# 第 8 轮实测证据（可复现）

这些都是**只读**产物：没有修改 ccusage 源码，没有改动本机 `~/.claude` / `~/.codex`。
运行 ccusage 时一律带 `--offline`，不联网、不写 HTTP 缓存。

## 复现步骤

```powershell
# 1) 取 LiteLLM 快照（build.rs 在 CCUSAGE_PRICING_JSON_PATH 缺省时会 panic）
$env:HTTPS_PROXY = "http://127.0.0.1:7897"   # 本机代理，按需替换
$rev = "d9123f459561754f41fd598a73b405b091d3bf84"   # flake.lock: nodes.litellm.locked.rev
Invoke-WebRequest "https://raw.githubusercontent.com/BerriAI/litellm/$rev/model_prices_and_context_window.json" -OutFile litellm-pinned.json

# 2) 构建（debug 足够，约 1 分 17 秒）
$env:CCUSAGE_PRICING_JSON_PATH = "$PWD\litellm-pinned.json"
cargo build -p ccusage --manifest-path <ccusage>\rust\Cargo.toml

# 3) 运行（本机真实数据，只读）
<ccusage>\rust\target\debug\ccusage.exe claude daily --json --offline
<ccusage>\rust\target\debug\ccusage.exe codex daily --json --offline
<ccusage>\rust\target\debug\ccusage.exe --json --by-agent --offline
<ccusage>\rust\target\debug\ccusage.exe session --json --offline
<ccusage>\rust\target\debug\ccusage.exe blocks --json --offline
<ccusage>\rust\target\debug\ccusage.exe claude daily --json --offline --mode display
<ccusage>\rust\target\debug\ccusage.exe claude daily --json --offline --no-cost

# 4) 离线重放
node replay-embed.js litellm-pinned.json embedded-keys.json   # 复现 build.rs 的裁剪 + 改名
node replay-match.js embedded-keys.json "claude-sonnet-4-20250514" "gpt-500" "opus550" ...
node query-md.js <ccusage>\rust\crates\ccusage-core\src\models-dev-pricing.json embedded-keys.json
node query-mdapi.js modelsdev-api.json <ccusage>\rust\crates\ccusage-core\src\models-dev-pricing.json

# 5) 格式化实验
rustc -O fmt-probe.rs -o fmt-probe.exe; .\fmt-probe.exe
```

## 文件

| 文件 | 作用 |
| --- | --- |
| `replay-embed.js` | 逐行复现 `build.rs:155-221`（`is_embedded_model` + `compact_pricing_json`） |
| `embedded-keys.json` | 重放产出的**等价内嵌键集**（534 键，来自 pin 版 LiteLLM 的 4084 条目） |
| `replay-match.js` | 复现 `pricing.rs:2667-2808` 的 `pricing_key_matches` / 边界与版本保护 / `max_by` |
| `query-md.js` | 查询仓库内 `models-dev-pricing.json`（3049 键）并与其做键集对照 |
| `query-mdapi.js` | 对照在线 `models.dev/api.json`（229 provider / 8830 条目 / 4043 去重 id） |
| `query-keys.js` / `query-full.js` | LiteLLM 键名检索（旧模型在哪些前缀下存在） |
| `fmt-probe.rs` | 复现 `output.rs:548-550` 的 `format!("${value:.2}")` 舍入行为 |

## 关键结果

- LiteLLM 4084 条目 → 白名单筛掉 3550 → **内嵌 534 键**；其中 `claude-*` 裸键 23 个（仅 4.5 代及以后）。
  复核员进一步**解压真实构建产物** `out/litellm-pricing.json.deflate`，与本目录的 `embedded-keys.json`
  **逐键、逐字段、逐值 0 差异**——重放等价由此确证。
- `claude-sonnet-4-20250514` / `claude-3-5-haiku-20241022` **无裸键**，靠模糊匹配落到
  `global.anthropic.…-v1:0` / `us.anthropic.…-v1:0`（区域 Bedrock 形态；`apac.` 不在内嵌表）。
- 版本边界保护的实证样例是 `gpt-5.4-9` 与 `claude-opus-5-9`（均无模糊命中）；
  `gpt-500` 是被边界检查挡住、`opus550` 是无子串关系——不能用它们证明版本保护。
- models.dev：内嵌 3049 键中 3021 键名仍在在线目录，28 个已消失（**仅比键名**，未比价格/档位）。
- `format!` 的 `{:.2}`：`0.015→$0.01`、`0.025→$0.03`、`2.675→$2.67`（按 f64 真值 round-half-to-even）。
- `ccusage daily --by-agent --json` 的**行级**键含 `agents`（选项生效）；
  `ccusage claude daily --by-agent` 报 `Unknown daily option '--by-agent'`（exit 2）。
  注意：检查时必须看**行级**键，顶层只有 `daily`/`totals`。
- `--mode display` 在本机（日志无 `costUSD`）给出 `totalCost = 0.0`；`auto` 与 `calculate` 数值相同。
- `cargo test -p ccusage-core --lib`：188 passed / 0 failed。

## 使用边界（重要）

1. `replay-match.js` **只实现了 `pricing_key_matches` 与最长键裁决，没有实现 `exact_only` 门禁**。
   因此它会把 `claude-opus-5-fast` 判成命中 `claude-opus-5`；真实运行时该名字由 models.dev
   的档位条目作答。用它下结论时必须叠加这一层。
2. 「3021 键名重叠」只是**集合比较**：价格数值、档位（tiers）、上下文上限、`exactOnly` 标记
   均未逐项比对。
3. 运行记录只包含**结构与字段名**；所有命令均带 `--offline`，未写 HTTP 缓存、未改任何数据目录。
