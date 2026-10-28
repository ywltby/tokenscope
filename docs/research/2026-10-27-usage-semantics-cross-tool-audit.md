# 十工具 token 用量读取口径交叉核实与对错裁决

> 核实日期：2026-10-27。核实对象：本目录九份用量统计工具调研（ccusage、TokenTracker、tokscale、
> UsageMesh、Token Monitor、codeburn、codex-token-pulse、TokenArena、cc-switch）记载的
> 「对相同 agent 的 token 用量读取口径」，外加 TokenScope 自身源码作为对照锚点。
> 本文回答两个问题：各工具对同一 agent 读出的数字是否一致；不一致处哪些符合实际、
> 哪些是确凿错误。
>
> **证据标记体系**（每条结论必带）：
> - 〔官方〕＝ 供应商官方文档本次联网核实（附 URL 与抓取日期）。
> - 〔实测〕＝ 调研文档记载的本机语料统计 / 沙箱复算 / 端到端测试实证（附 `文档:行号`）。
> - 〔源码〕＝ 调研文档的源码级拆解结论（附 `文档:行号`；ccusage 与 cc-switch 两份经
>   多轮对抗式复核，其余经其文档内自查）。
> - 〔本仓〕＝ TokenScope 自身源码或 `docs/stats-semantics.md` 核实（附 `文件:行号`）。
> - 〔推导〕＝ 由两条已证事实逻辑推出（写明推导链）。
>
> 行号均指各调研文档自身实际行号。本文档全部裁决级行号已于 2026-10-27 回查原文档验证，
> 抽查记录与两处转述修正见 §12。

## 1. 结论摘要

1. 「不同工具对同一 agent 读出相同 token 数字」不成立：十个实现除「input 语义家谱」外，
   在缓存写档位、reasoning 归属、sidechain、去重、容错、数据源范围上全部存在分歧；
   同一工具跨版本也在漂移（§9）。
2. 唯一的全局共识是 input 语义家谱：Anthropic 家直取不扣（§4）、OpenAI 家必扣
   （§2、§5）、Gemini 家 input 含 cached（§2、§6）。
3. 确凿错误集中在三处：TokenArena 的漏桶与双计（cache_creation 全漏、uuid 去重无效、
   Codex 差分基准缺陷，§4.6/§4.12/§5.7-§5.8）、TokenTracker 无 message.id 完全不去重
   （§4.13）、ccusage 的 `.jsonl.zst` 漏文件（§5.14）；codeburn 与 cc-switch 的 Gemini
   thoughts 加回/并入是存疑级双计（取决于上游注释是否适用于其目标版本，§6）。
   其余分歧多为合法口径取舍（§4–§7）。
4. TokenScope 的扣减式与守恒校验经官方语义对照全部站得住；唯一实质差距是 Claude
   1h 缓存写档未分（token 对、1h 写费相对正确费用低估 37.5%，即正确费用比估算高 60%）；不做 total 差分与 reasoning
   不单列经裁决属保守取舍而非错误（§10）。

## 2. 证据准绳：官方语义核实记录

三条裁决准绳按效力排序：官方 API 语义 > 上游客户端真实写日志行为 > 工具间互证。

- **Anthropic 缓存写定价**〔官方〕：抓取
  <https://platform.claude.com/docs/en/about-claude/pricing>（2026-10-27，HTTP 200）。
  定价表确认全系：5m 写 = base input × 1.25，1h 写 = base input × 2（Opus 5.5：
  $4 → $5 → $8；Sonnet 5.5：$2 → $2.5 → $4；Haiku ≤100k：$0.1 → $0.125 → $0.2）；
  页面同时确认 1h TTL 作为产品特性存在。这一条直接裁决 §4 的缓存写档位之争。
- **OpenAI Responses 语义**（input_tokens 含 cached；reasoning 含在 output）：
  官方页需 JS 渲染未能直接抓取，改用双源〔本仓〕+〔实测〕闭环——
  `crates/tokenscope-core/src/source/codex.rs:9-15` 引官方示例
  `15000 = 12000(cached) + 3000(cache_write) + 0`，且 `docs/stats-semantics.md:99-114`
  以守恒式固定该语义；TokenTracker 对上游注释逐字引用「cached slice is INSIDE the input
  count」并以同一批 rollout 与 ccusage 按日数字对账实证不减法膨胀 6–7 倍
  （`2026-10-14-tokentracker-usage-statistics.md:2016-2022`，下称 TT:2016-2022）；
  reasoning 子集关系有本机 65381 对样本零反例（规则定义与计价置 0 在 TT:2051-2054，
  实测统计在 TT:2070 与附录 E1 的 TT:4188）。
- **Gemini thoughts 语义**：官方 UsageMetadata 参考页两次抓取均被导航截断，未取得
  直接文本。裁决改以一手源码注释为准：TokenArena 文档引用 gemini-cli.ts:208-209
  「Gemini 的 output 已含 thoughts、input 已含 cached，减掉避免重复计数」
  （`2026-10-15-tokenarena-usage-statistics.md:165-168`，下称 TA:165-168）；
  ccusage 的条件公式 `total == input+output+thoughts+tool`（cached 不入 total）
  与该注释同向（`2026-10-14-ccusage-usage-statistics.md:1244`，下称 ccusage:1244）。
  最终裁决仍以实测原始日志为准（§6）。

## 3. 覆盖矩阵

| 实现 | Claude Code | Codex | Gemini CLI | OpenCode | 其他 |
| --- | --- | --- | --- | --- | --- |
| ccusage v20.0.31 | 读 | 读 | 读 | 读 | 共 18 适配器 |
| TokenTracker v1.3.14 | 读 | 读 | 读 | 读 | — |
| tokscale v4.18.0 | 读 | 读 | 读 | 读 | — |
| UsageMesh（内嵌 tokscale-core v4.14.0） | 读 | 读 | 读 | 读 | 50 客户端注册表 |
| Token Monitor（tokscale fork fa5f0c13） | 读 | 读 | 读 | 读 | fork 专属 3 客户端 |
| codeburn v0.9.27 | 读 | 读 | 读 | 读 | 共 43 provider |
| codex-token-pulse | 读 | 读 | 不读 | 不读 | Grok/OpenCodex 等 5 类（总表 7 类应用、10 载体） |
| TokenArena | 读 | 读 | 读 | 读 | 共 20 agent |
| cc-switch v4.0.21 | 读 | 读 | 读 | 读 | Grok/Pi/MiniMax/agy 共 9+ |
| TokenScope | 读 | 读 | 不支持 | 不支持 | WorkBuddy ×2 |

依据：各调研文档范围与方法节（ccusage:1-56、TT:1-20、`2026-10-14-tokscale-usage-statistics.md:1-20`、
`2026-10-14-usagemesh-usage-statistics.md:1-20`、`2026-10-13-token-monitor-usage-statistics.md:1-20`、
`2026-10-14-codeburn-usage-statistics.md:1-14`、`2026-10-15-codex-token-pulse-usage-statistics.md:129-141`、
TA:1-12、`2026-10-11-cc-switch-usage-statistics.md:1-12`）。PULSE 不读 Gemini/OpenCode 的
排除性证据：PULSE:129-141（来源总表 10 载体行归并 7 类应用，其中 Codex 占 4 载体）、
PULSE:339-341（§3.12 只读固定路径）。

## 4. Claude Code 逐条裁决

| # | 分歧点 | 裁决 | 依据 |
| --- | --- | --- | --- |
| 4.1 | input 直取不扣（全体一致） | ✅ 符合实际 | 〔官方〕Anthropic 四桶互斥语义；TK 文档明说两种语义不可比（tokscale:295-296）；无任何工具做反向扣减 |
| 4.2 | ccusage 区分 5m/1h、1h 按 input×2 计价 | ✅ 符合实际 | 〔官方〕§2 定价表；〔源码〕ccusage:529-536（`CACHE_CREATE_1H_INPUT_MULTIPLIER=2.0`，无明细对象时扁平全按 5m 价、1h 桶为 0） |
| 4.3 | codeburn 1h 写价 = 5m 写价 × 1.6 | ✅ 符合实际（官方比例下与 4.2 等价） | 〔官方〕1.25×1.6=2.0；〔源码〕CB:804-806 自述「写价取兜底 1.25×input 时等价 2.0×」，显式写价条目按各自 5m 价 ×1.6 |
| 4.4 | PULSE 读到 ephemeral 但扁平优先、明细求和仅兜底、不分档 | ⚠️ token 对、1h 写费低估 | 〔源码〕PULSE:505-511（仅当扁平 ≤0 且有 dict 才对 5m+1h 求和） |
| 4.5 | CCS / TokenTracker / TokenScope 只读扁平字段 | ⚠️ token 对、1h 写费低估（1.25 单价按 1h 计：示例价 $6 vs 估算 $3.75，相对正确费用低估 37.5%；反过来说正确费用比估算高 60%）；CCS:363「1h 分档只用于计价」不改变结论——其解析层四桶直取、无 ephemeral_1h 明细，分档没有 token 可分 | 〔源码〕CCS:362-363、TT:2812；〔本仓〕claude.rs:58-68；TS 已被 ccusage:3365 点名「最值得优先评估」 |
| 4.6 | TokenArena 完全不映射 `cache_creation_input_tokens` | ❌ 确凿错误：缓存写 token 全漏，总量与费用双低估 | 〔源码〕TA:138 |
| 4.7 | UsageMesh（v4.14.0）无 1h 桶 | ⚠️ 版本事实：1h 桶 v4.18.0 才引入 | 〔源码〕UM:1741-1748 |
| 4.8 | sidechain 计入（ccusage/TT/CB/TokenScope） | ✅ 符合实际：子代理是真实 API 调用 | 〔源码〕CB:345-346；〔本仓〕claude.rs:5-13、242-247 |
| 4.9 | PULSE 解析层完全忽略 isSidechain | ⚠️ 不直接双算（message.id 三级键兜底），少父文件内 sidechain 行的 requestId 级合并 | 〔源码〕PULSE:526-535 |
| 4.10 | TK 系 sidechain 行 session_id 归父会话 | ⚠️ 只影响会话计数，不影响 token | 〔源码〕tokscale:300-304 |
| 4.11 | 同 message.id 跨文件/流式多行是真实场景 | ✅ 已证 | 〔实测〕ccusage:439-440 端到端测试：父 10 + sidechain 50000，四报表总计必须等于 32 |
| 4.12 | TokenArena 用行级 uuid 去重 | ❌ 确凿错误：uuid 每行唯一，对同 message.id 多行无效 → 双计 | 〔源码〕TA:134-138；〔实测〕4.11 |
| 4.13 | TokenTracker 无 message.id 完全不去重 | ❌ 确凿错误（同 4.12 类）；其旧版「要求 msgId 且 reqId」缺陷曾实测 1.6–3.7 倍超算 | 〔源码〕TT:1736-1742、1754 |
| 4.14 | ccusage memmem 精确匹配 `"usage":{`，带空格写法整行静默丢弃 | ⚠️ 正常数据不触发（官方 `JSON.stringify` 无空格）；静默无计数是工程缺陷 | 〔源码〕ccusage:306-320 |
| 4.15 | synthetic 行处理（ccusage 计入但隐藏 vs TokenScope 跳过+计数） | ✅ token 与费用等价（本机语料 synthetic 行 usage 全零）；请求数口径或有差，同 4.16 | 〔实测〕TT:1798-1807（6 行全零）；〔本仓〕claude.rs:28、299-304 |
| 4.16 | 全零行（CCS 过滤不导入 vs TokenScope 照常产事件） | ✅ 合法取舍：token 不受影响，只差请求数口径 | 〔源码〕CCS:359-361；〔本仓〕claude.rs:258-330 |
| 4.17 | 坏行逐项计数（仅 TokenScope SF08 与 TK 系部分路径有） | ✅ TokenScope 正确；其余静默丢行属工程缺陷 | 〔本仓〕claude.rs:258-330；〔源码〕ccusage:3752-3758 自认无计数 |

## 5. Codex 逐条裁决

| # | 分歧点 | 裁决 | 依据 |
| --- | --- | --- | --- |
| 5.1 | input 必扣 cached（全体一致） | ✅ 符合实际 | 〔官方〕§2 OpenAI 语义；〔实测〕TT:2016-2022 不减法 6–7 倍费用膨胀（与 ccusage 对账） |
| 5.2 | TokenScope `raw−cached−cache_write` 三桶拆分 + 守恒校验 | ✅ 最完整：与官方五分量对齐 | 〔本仓〕codex.rs:9-15、555-563；stats-semantics.md:99-114 |
| 5.3 | ccusage 输出层扣 `cached+cache_creation` 并钳制上界 | ✅ 符合实际且防溢出 | 〔源码〕ccusage:866-874 |
| 5.4 | TokenTracker input 只减 cached、不减 cache_write | ❌ 条件性缺陷：cache_write 非零时该部分既留在 input 又单列 → 双计；订阅流 cache_write 恒 0 时无害 | 〔源码〕TT:2025-2027；〔本仓〕stats-semantics.md:116-121（订阅流恒 0 属数据源现状） |
| 5.5 | TK 系 Codex cache_write 恒 0 | ⚠️ 丢弃真实字段，缓存写费用漏计；订阅流数据下无损 | 〔源码〕UM:1071-1086 |
| 5.6 | total 差分兜底（ccusage/CCS/TK 系/CB/PULSE/TT 各自实现） | ✅ 方向正确（对付累计重置/乱序） | 〔实测〕TT:1989 把 total 逐条累加重复 ≈2128 倍，累计字段只能取末值 |
| 5.7 | TokenArena 差分基准只在回退分支回写 | ❌ 确凿错误：同文件混用 last/total 时已计量被整额再计（自认缺陷） | 〔源码〕TA:158-160、1036-1040 |
| 5.8 | TokenArena 首见 total 直接采信累计值 | ❌ 确凿错误：纯累计文件把首见前累计归到首见日（日期归属错）；混用文件整额重复计 | 〔源码〕TA:144-145、1039-1040 |
| 5.9 | TokenScope 只取 last、不做 total 差分 | ⚠️ 保守取舍：last 缺失的非压缩场景少算，量级小 | 〔本仓〕codex.rs:4-8 |
| 5.10 | token_usage_record：CCS 补记 / TokenScope 挂起配对 / ccusage 须 compacted 认领 / CB 压制 token_count / TT 作 compaction 侧信道 | ✅ 五家方向都对准同一真实场景（Codex 0.153+ 远程压缩） | 〔实测〕CCS:705 本机 68 rollout 中 44 条压缩漏计（输入 0.22%/输出 1.18%）；〔源码〕CCS:699-715、ccusage:955-967、CB:428-435、TT:1991-2002；〔本仓〕codex.rs:379-405（挂起配对实现） |
| 5.11 | TK 系 / PULSE / TA 未处理 token_usage_record | ⚠️ 真实漏计约输出 1%（压缩场景） | 〔实测〕同 5.10 的 44 条量级；〔推导〕CCS 实测占比外推 |
| 5.12 | reasoning 已含 output（ccusage/TokenScope/TT/CB/PULSE） | ✅ 符合实际 | 〔实测〕TT:2070、TT:4188（附录 E1）65381 对零反例（`reasoning>0` 60715 对、比值上界 1.0000）；〔源码〕TT:2051-2054 规则定义与计价置 0；PULSE 不单独展示但 output 直取不少算（PULSE:448） |
| 5.13 | TK 系 / TA 把 reasoning 拆出独立桶 | ⚠️ 重定义列而非错误：费用中性（OpenAI reasoning 与 output 同价），但跨工具 output 列不可比 | 〔推导〕5.12 + 各家计价同价；TK 警告「直接并计会双计」（tokscale:352-353） |
| 5.14 | ccusage 不支持 `.jsonl.zst` | ❌ 确凿错误：新版 Codex 压缩转录整文件漏掉 | 〔源码〕ccusage:801；tokscale:3-5（上游专门提交支持，证明文件真实出现） |
| 5.15 | PULSE 不读 `CODEX_HOME` | ⚠️ 覆盖缺口：自定义环境变量的用户数据漏读 | 〔源码〕PULSE:147-148 |

## 6. Gemini CLI：一处直接矛盾的三方裁决

同一字段（thoughts 与 output 的关系）存在三种互斥解读：

| 方 | 做法 | 对上游注释「output 已含 thoughts」的相容性 |
| --- | --- | --- |
| TokenArena | `input−cached`、`output−thoughts` 裸减 | ✅ 正是注释的指示（TA:165-168，引 gemini-cli.ts:208-209） |
| ccusage | 只在 `total==input+output+thoughts+tool` 时减 cached；thoughts 桶原样 | ✅ 自洽（total 不含 cached 的结构判断与注释同向，ccusage:1244） |
| TokenTracker / TK 系 | thoughts 单独成桶、不并入 output | ✅ 费用中性（reasoning 与 output 同价时），列口径不同（TT:1034；tokscale:382-386） |
| codeburn | Gemini 不在 output-已含白名单，计费 output+reasoning 加回 | ❌ 若注释适用则 thoughts 双计 |
| cc-switch | thoughts 并入 output 统计 | ❌ 同上（CCS:427-430） |

裁决：**初步判 TA/ccusage/TT/TK 方向正确；CB 与 CCS 的「加回/并入」在注释适用范围内
是双计错误**——但注释取自 gemini-cli 特定版本源码，CB 面向 43 家 provider 的白名单
可能针对不同版本，最终以实测原始 chats JSON 为最硬准绳（本机未装 Gemini CLI，本次
无法实测，列为待验）。另：TA 裸减无 `Math.max` 兜底，cached>input 时整条丢弃
（TA:168-169、1041-1046）——正常数据 cached≤input 恒成立，属防御缺失而非现行错误；
TK 系 Gemini 无 dedup_key，跨扫描目录重复必双计（tokscale:390-397），会话一文件布局下
风险低。

## 7. OpenCode

已核实各家一致：input 本就是纯非缓存输入、直取不扣（TA:247-248、TT:1044）。
✅ 符合实际。残余差异：UM 的 strict_cache（read/write 缺一整行丢弃，UM:1141）属
防御性取舍；TokenTracker 的 SQLite 支路与 JSON 支路相加存在双计面（TT:1048），
本文档未展开验证。

## 8. 定量教训（判断对账差异是否正常的标尺）

| 现象 | 倍数 | 出处 |
| --- | --- | --- |
| Codex 把累计字段逐条累加 | ≈2128 倍重复 | 〔实测〕TT:1989（本机 64 rollout：total 求和 9,740,950,476,478 vs 增量求和 4,578,527,502） |
| Codex 不做 `input−=cached` | ≈6–7 倍费用膨胀 | 〔实测〕TT:2016-2022（与 ccusage 按日对账） |
| Claude 旧去重键要求 msgId 且 reqId | 1.6–3.7 倍超算 | 〔实测〕TT:1754（issue #64） |

## 9. 同源漂移：tokscale 三版本

tokscale v4.14.0 → v4.18.0 的读取行为变化有逐条 git diff 证据（UM §3.8）：
`cache_write_1h` 桶新增（v4.18.0 独有）、Codex zstd 支持（提交 bd62b8c4）、Claude
advisor 计入（提交 c8fc84b）、客户端注册表 50→56、`CACHE_FORMAT_VERSION` 5→10、
Codex parser_version 7→14。fork fa5f0c13 基于上游 v4.18.0（TM:9-10），另加
proma/catpaw/qodercn 三个专属客户端——走 clap 解析前的摘出通路，不进上游 56 id
注册表与 `--client` 取值表（TM:371、422-424、226-228），桶语义随上游基线。
结论：**口径对比必须钉到具体版本**；UsageMesh 与 Token Monitor
各自钉住不同版本，互相之间不可直接对数。

## 10. TokenScope 自身定位

**经官方语义对照站得住的**：Codex 扣减式 `raw−cached−cache_write` + 守恒校验
（§5.2，十家唯一做三桶拆分且守恒的）；token_usage_record 挂起配对（§5.10，与 CCS
同发现，2026-10-22 补齐，stats-semantics.md:89-93）；坏行逐项计数与受检算术
（§4.17，SF08，超过 ccusage/CCS/TT 的静默丢行）；项目/cwd 身份模型（九家工具中
CCS 明确不采集，CCS:599）。

**唯一实质差距**：Claude 1h 缓存写档未分（§4.5）——token 对、1h 写费相对正确费用低估 37.5%（即正确费用比估算高 60%）。
修法明确：解析层加读 `cache_creation.ephemeral_1h_input_tokens` 明细对象，价格侧
1h=input×2 已有官方依据（§2）。注意 ccusage:3366 所说 TokenScope「明确忽略
token_usage_record」已过时，现行口径为挂起配对（stats-semantics.md:73-84）。

**经裁决属保守取舍、不必视为缺陷的**：不做 Codex total 差分（§5.9）；reasoning
不单列（§5.12-5.13，费用中性）；Codex 跨源身份保守四桶指纹（token_count 无请求
标识，不猜测，stats-semantics.md:233-244）。

## 11. 对账顺序

两个工具对不上时按 TT:3570 给出的顺序排查（与本文结论互相印证）：
时区基准 → 去重语义 → token 口径 → 来源范围 → 账号视图 → 成本来源。
前两层占绝大多数差异；对账前必须先钉双方版本（§9）。

## 12. 证据可信度声明

本文档行号转引自 2026-10-27 四组并行提取；全部裁决级行号（§4-§6 共 30 余处）已
回查原文档逐一验证，命中情况与修正如下：

1. **修正一（Gemini 矛盾归属）**：一手注释「output 已含 thoughts」在 **TokenArena**
   文档（TA:165-168，其源码引 gemini-cli.ts:208-209）；初稿误记为 TokenTracker
   （TT:166-169 实为写队列管线描述）。TokenTracker 的实际做法是 thoughts 不并入
   output（TT:1034）。§6 按修正后归属裁决。
2. **修正二（CB 1h 系数等价性前提）**：1h=5m×1.6 与 1h=input×2 的等价在兜底价
   1.25×input 下成立；显式写价条目按各自 5m 价 ×1.6（CB:804-806）。§4.3 已限定。
3. **精确化一（PULSE ephemeral）**：PULSE 优先扁平字段，明细求和仅作兜底，与
   ccusage 的「优先明细」方向相反（PULSE:505-511 vs ccusage:529-536）。
4. **精确化二（TA 首见采信）**：纯累计（无 last）文件不高估，只是把首见前累计
   归到首见时间点；混用 last/total 的文件才整额再计（TA:1039-1040）。
5. **精确化三（外部复核反馈，2026-10-27）**：「65381 对零反例」的出处细化——
   TT:2051-2054 是子集规则定义与计价置 0 的实现，实测统计与数值表在同小节
   TT:2070 与附录 E1 的 TT:4188。已回查两处原文验证后拆分标注（§2、§5.12）。
6. **审查修正（2026-10-28 外部审查）**：回查九份调研文档与本仓源码后修正——
   §5.10 的「codex 源码配对逻辑 codex.rs:379-405」实为本仓 TokenScope 挂起配对
   实现，标记由〔源码〕改〔本仓〕；§9 的 TM:1260-1262 系 .jsonl.zst 实证而非
   fork 客户端依据，改引 TM:9-10、226-228、371、422-424；§7 删除 CB:176
   （provider 路径清单，不承载 OpenCode 缓存语义，codeburn 文档全文无该口径
   段落）；§1 结论 2/3 与正文裁决集合对齐（补 §4.13，codeburn/cc-switch 的
   Gemini 双计单列存疑级）；§4.5 补 CCS:363 半句说明；§4.15 限定 token/费用
   等价维度并修正 claude.rs:29-31 → 28、299-304；§5.2 区间 544-559 → 555-563
   （补 validate_buckets 校验行）；§3 PULSE 行标注 10 载体/7 类应用口径。

未回查、仅作背景引用的行号（§3 覆盖矩阵、§8 教训表、§9 漂移清单的部分条目）
保持〔转述〕级，采信前可按需回查。官方核实的两条 URL 与抓取结果见 §2；
Gemini UsageMetadata 官方页未取得直接文本，相关裁决的证据等级为〔源码〕（一手注释）
而非〔官方〕，已在 §6 标注。
