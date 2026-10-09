# 会话目录（cwd）变化语义核验（B01）

> 只读核验：先在本机真实会话日志上统计，再用**合成样例**表达结论。文档只保留
> 最小结构化字段（type / sessionId / cwd / 行序）与聚合计数，不含真实路径、
> 提示词、工具输出或用户内容；探针脚本只在本机临时目录运行，不入库。

## 1. 方法与样本

- 对象：本机 `~/.claude/projects/**/*.jsonl`（20 个 session）与
  `~/.codex/sessions/**/*.jsonl`（56 文件 / 66 个 session）；只读扫描，不写入
  任何被扫描目录，也不改动 TokenScope 缓存。
- 口径：按行提取结构化字段——Claude 顶层 `cwd` / `sessionId` / `type` /
  `isSidechain`；Codex `session_meta`、`turn_context` 的 `payload.cwd`——按
  session 分组比较 cwd 序列，变化符号化：`F` 回到首值、`S` 首值的子目录、
  `P` 首值的父目录、`O` 其他目录、`=` 不变。
- 局限：单机样本、无可控实验（不能要求真实切换目录再发请求）、无法观测
  Claude Code 进程自身的启动参数；因此下列结论区分「已验证」「一致解释」
  「未验证推论」三档。

## 2. Claude 侧观测（已验证）

- `cwd` 出现在多种行型：assistant 1046 行、user 795 行、attachment 300 行、
  system 24 行（本机计数）——**每行都带**，是逐行状态快照，不是只在会话头部
  写一次。
- 单 session 内 cwd 恒定是常态：20 个 session 中 16 个只有一个取值。
- 出现多个取值的 4 个 session 中 **3 个是纯 sidechain 文件**（例如 86/86、
  91/91、78/78 行全部 `isSidechain: true`），变化形态为 `P` 或 `O`——即子代理
  记录使用与主线不同的工作目录。
- 唯一一个非 sidechain 漂移 session（337 行）：形态 `S F`——第 9.2% 处进入
  首值的子目录，第 16.4% 处回到首值；两次变化都发生在 `user` 行、前一行均为
  `assistant`，其后 83% 的行保持首值。
- 结论 1：Claude 的 `cwd` 是行级快照，主线漂移在本机表现为「短暂进入子目录
  再返回」。
- 结论 2：sidechain 行的 cwd 与主线不同属常态 → 身份采样必须排除 sidechain
  （阶段 A 已按此实现，见 `src/source/claude.rs`）。
- 结论 3（一致解释）：若 `cwd` 会随 Bash 工具内的 `cd` 变化，应出现频繁且与
  工具调用成对的变化；实测变化稀疏（4/20，其中 3 个来自 sidechain）且长区间
  稳定——与「随 shell 子进程 `cd` 变化」不符，更符合「随会话进程工作目录
  变化」。**未验证推论**：结构化字段不因工具内 `cd` 文本而改变。

## 3. Codex 侧观测（已验证）

- `session_meta` 66/66 带 `cwd`；`turn_context` 905/905 带 `cwd`，无空值、
  无类型异常（与阶段 A 的容错实现一致：真实样本没出现坏值，但合成 fixture
  仍须覆盖）。
- 单 session 内 cwd 切换罕见：66 个 session 中 1 个（6 条记录中的第 5 条
  `turn_context`，形态 `O`，完全切换目录）。
- 结论 1：`turn_context` 是**轮上下文快照**（同一行还带 `model` 等），其 `cwd`
  表示后续轮次的工作目录 → **轮级更新语义**，与 Claude 的行级快照不同。
  这正是计划强调的「不能仅凭存在 cwd 字段认定两侧更新语义相同」。
- 结论 2：`session_meta` 恒带 cwd，可作为新会话初始上下文；本机样本未出现
  缺失 cwd 的 `session_meta`（合成 fixture 覆盖该分支）。

## 4. 归属规则待决实例（需产品确认）

阶段 A 已实现的现状：Claude 按**会话首个有效 cwd**归属（sidechain 不参与），
Codex 按**当前轮次 cwd**归属（A → B → A 保留，`session_meta` 重置）。下列实例
给出各选项的结果，请选择后由 B02/B03 落实。

### 实例 1：Claude 会话内短暂进入子目录（本机真实出现过的形态）

```jsonl
{"type":"user","sessionId":"s1","cwd":"C:/work/alpha"}
{"type":"assistant","sessionId":"s1","cwd":"C:/work/alpha"}
{"type":"user","sessionId":"s1","cwd":"C:/work/alpha/sub"}
{"type":"assistant","sessionId":"s1","cwd":"C:/work/alpha/sub"}
{"type":"user","sessionId":"s1","cwd":"C:/work/alpha"}
{"type":"assistant","sessionId":"s1","cwd":"C:/work/alpha"}
```

- 选项 A（保持阶段 A）：三个请求全部归 `C:/work/alpha`。
- 选项 B（事件级精确分组）：请求 1/3 归 `C:/work/alpha`，请求 2 归
  `C:/work/alpha/sub`；**不做父子目录归并**，于是同一仓库出现两行。
- 选项 C（事件级 + 子目录政策）：请求 2 也归 `C:/work/alpha`（「A → A/sub 不算
  切换」），A → B（完全不同目录）才算切换；实现需要"同项目根"的可靠判据
  （父子关系不足以判定，需要显式规则或映射）。

### 实例 2：Claude 会话内切换到完全不同的目录

```jsonl
{"type":"user","sessionId":"s1","cwd":"C:/work/alpha"}
{"type":"assistant","sessionId":"s1","cwd":"C:/work/alpha"}
{"type":"user","sessionId":"s1","cwd":"D:/other/beta"}
{"type":"assistant","sessionId":"s1","cwd":"D:/other/beta"}
```

- 选项 A：两个请求都归 `C:/work/alpha`（B 阶段不改变行为）。
- 选项 B：请求 1 归 `C:/work/alpha`，请求 2 归 `D:/other/beta`（精确分组，
  两行项目）。

### 实例 3：Codex 轮次切换（现行为，供对齐讨论）

```jsonl
{"type":"session_meta","payload":{"session_id":"s1","cwd":"C:/work/alpha"}}
{"type":"turn_context","payload":{"model":"m","cwd":"C:/work/alpha"}}
{"type":"event_msg", "... token_count ..."}
{"type":"turn_context","payload":{"model":"m","cwd":"D:/other/beta"}}
{"type":"event_msg", "... token_count ..."}
{"type":"turn_context","payload":{"model":"m","cwd":"C:/work/alpha"}}
{"type":"event_msg", "... token_count ..."}
```

- 现行为（保留）：轮级精确分组，A → B → A 保留，三个请求分别归 alpha / beta /
  alpha。
- 若要与 Claude 对齐成「会话初始路径冻结」，需改为按 `session_meta` 冻结——
  本机证据显示这是较少见的路径（1/66 session 出现过切换），且会丢掉已实现的
  切换能力。

### 实例 4：缺上下文

```jsonl
{"type":"session_meta","payload":{"session_id":"s2"}}
{"type":"turn_context","payload":{"model":"m"}}
{"type":"event_msg", "... 请求 1 ..."}
{"type":"turn_context","payload":{"model":"m","cwd":"E:/third/gamma"}}
{"type":"event_msg", "... 请求 2 ..."}
```

- 现行为（保留）：请求 1 记 `(未知)`，请求 2 归 `E:/third/gamma`；不回溯。
- 备选：把请求 1 也归 `E:/third/gamma`（用首个出现的 cwd 回填整段会话）——
  需明确是否允许这种回溯，本计划默认**不允许**（不猜测历史归属）。

## 5. 转换为 B03 的具名 fixture

| 用例 | 来源实例 | 合成 fixture（B03 创建） |
| --- | --- | --- |
| `claude_structured_cwd_switch` | 实例 1、2 | `tests/fixtures/project-path/session-switch/claude-*.jsonl` |
| `codex_structured_cwd_switch` | 实例 3 | `tests/fixtures/project-path/session-switch/codex-*.jsonl` |
| `switch_does_not_reassign_previous_events` | 三个实例共同要求 | 同上（断言切换前后的归属互不影响） |
| `shell_cd_text_does_not_change_project` | 结论 3 的反例设计 | 含 `tool_use` 文本 `cd ...` 的合成行 |
| `both_agents_follow_decided_subdirectory_policy` | 实例 1 的选项结果 | 两侧同构的最小子目录样例 |

## 6. 结论与建议

- 两侧语义**确实不同**：Codex 的 `turn_context.cwd` 是轮级工作目录；Claude 的
  顶层 `cwd` 是行级状态快照，且 sidechain 常态带不同目录。
- 最小、可解释的下一步是：把实例 1/2 的选项确认下来，再决定 B02 的数据模型
  （`session_initial_cwd` 与 `event_cwd` 是否需要同时上线）与 B03 的归属规则。
- 若选择选项 B（事件级精确分组），必须同时接受「同一仓库因短暂子目录而分成
  两行」以及「不做父子目录归并」的后果；仓库级项目的还原需要可靠根身份或
  显式映射，不在本阶段。
