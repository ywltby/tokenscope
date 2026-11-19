// R04（全计划审核 Task 4）：视图快照保存队列。
// 模块级——App 用 v-if 切换汇总/设置页，Dashboard 卸载不能丢保存顺序；
// 合并待保存最新项 + 串行派发：绝不并发 fire-and-forget 让旧写覆盖新写。
import { invoke } from "@tauri-apps/api/core";

/** 快照格式版本：v22 = 跨渠道别名合并 + CCS 日汇总下钻行（v0.9.4）——
 * 同一模型的不同渠道写法归一为一个分组键，纯日汇总模型的下钻从 0 行变为
 * 按统计日的 CCS 日行，「未知项目」下钻也纳入日行；v21 快照的分组行数、
 * 下钻 key 与明细行数都不再可信（升级后首屏可能继续显示两行同名模型或空的
 * CCS 明细，首次查询失败时还会继续回显），读取时忽略重建。v0.9.3 写下的
 * 快照同为 v21，只能靠格式版本而非发行版本区分；
 * v21 = 日汇总保留按 0 估算披露（v0.9.2 第十一轮）——金额
 * 一致但任一可能胜出路径按 0 估算时，`complete` 与
 * `cache_price_zero_estimated` 读数改变，v20 及更早快照的费用依据状态不再
 * 可信，读取时忽略重建；
 * v20 = 日汇总跨候选择价 + 语义完整性（v0.9.2 第十轮）——
 * 择价单元改为「价格候选 × 该时刻适用规则」同池竞争（跨候选的请求配比
 * 不再按日总量猜），完整性按支持集实际使用分项与分档估算状态判定；
 * 部分汇总的费用/未定价/估算读数改变，v19 及更早快照的费用读数不再可信，
 * 读取时忽略重建；
 * v19 = 日汇总请求用量比例判定（v0.9.2 第九轮）——可能胜出
 * 规则改按「任意正 token 配比下的线性费用可行性」判定（凸包支配），修正
 * 等权配比漏掉 skewed 请求的问题，部分汇总的费用/未定价读数改变，v18 及
 * 更早快照的费用读数不再可信，读取时忽略重建；
 * v18 = 日汇总时区偏移切换点 + 按请求构成判定可能胜出规则
 * （v0.9.2 第八轮）——时段切分加入真实偏移切换点（前拨日的价格窗口不再被
 * 漏掉），分项判定改为「在用到该分项的可行请求里哪些规则可能胜出」，
 * 部分汇总的费用/未定价读数改变，v17 及更早快照的费用读数不再可信，读取
 * 时忽略重建；
 * v17 = 日汇总分项保留 + DST 回拨重复小时（v0.9.2
 * 第七轮）——择价改为逐分项可证明判定（部分定价规则的已知分项、交叉优势
 * 规则的公共不变分项保留），时段切分覆盖回拨日重复本地时刻的两个映射，
 * 部分汇总的费用/未定价读数改变，v16 及更早快照的费用读数不再可信，读取时
 * 忽略重建；
 * v16 = 日汇总时段区间划分 + 逐请求主导择价（v0.9.2
 * 第六轮）——来源日按真实绝对时间边界切分、择价改用支配判据，部分汇总
 * 的费用/未定价读数改变（部分重叠与交叉优势规则转为未定价、跨时区窗口
 * 不可发生者转为可证明金额），v15 及更早快照的费用读数不再可信，读取时
 * 忽略重建；
 * v15 = 日汇总日期可行性/规则择价/same_as_input（v0.9.2
 * 第五轮）——部分此前误判为未定价的汇总会得到可证明金额，v14 的费用
 * 读数不再可信，读取时忽略重建；
 * v14 = 日汇总费率适用性 + 费用依据状态（v0.9.2 第四轮）——
 * 分段/时段依赖缺失条件的分项改为真实未定价，`has_estimated_cost` 取代
 * 「找到候选」判定；v13 的费用读数与依据状态不再可信，读取时忽略重建；
 * v13 = 日汇总费用覆盖判定（v0.9.2）——unknown_tokens/
 * unknown_pricing 的语义从「汇总 token 全部未定价」改为「实际未获费用
 * 覆盖」；旧快照的费用状态列与未知读数不再可信，读取时忽略重建；
 * v12 = 「时间预设」取代固定日期区间（`filters.preset`），
 * 且快照**不再包含、也不再写入时区**——时区是 settings.toml 的单一来源，
 * 快照恢复不得改写它；v11 = 「只看请求明细」（rollups_mode）随快照保存/恢复，
 * v11 及更早快照读不出该模式（恢复后回退 auto、可能重新带入日粒度历史），
 * 读取时忽略；v10 = 上下文解析修复（第三轮审查）——Codex 会话边界、
 * file URI 点段、UNC/URI 主机大小写与 POSIX 尾空格的修复改变了事件的
 * session/project，v9 及更早快照的分组与下钻 key 不再可信；
 * v9 = 模型匹配规则（等价键）；v8 = 项目根归并；v7 = 统一项目身份；
 * v6 = 查询会话契约（SF04）；v5 及更早版本不含会话身份且游标为旧格式。 */
export const SNAPSHOT_VERSION = 22;

export type SnapshotDispatch = (payload: unknown) => Promise<void>;

const defaultDispatch: SnapshotDispatch = (payload) =>
  invoke("view_cache_save", { value: payload });

let tail: Promise<void> = Promise.resolve();
let latest: unknown = null;
let scheduled = false;
let lastError: unknown = null;

/**
 * 入队一次快照保存。重复入队时只保留最新 payload（合并）；派发严格按
 * 入队顺序串行执行——先入队的写完成后才派发后入队的，磁盘最终状态 =
 * 最新一次入队的内容，与完成时序无关。
 * dispatch 可注入（测试用 deferred 控制完成时序）；IPC 失败只记警告，
 * 不产生未处理 rejection，也不阻止新鲜数据显示。
 */
export function enqueueSnapshotSave(
  payload: unknown,
  dispatch: SnapshotDispatch = defaultDispatch,
): void {
  latest = payload;
  if (scheduled) return;
  scheduled = true;
  tail = tail.then(async () => {
    try {
      while (latest != null) {
        const p = latest;
        latest = null;
        try {
          await dispatch(p);
          lastError = null;
        } catch (e) {
          lastError = e;
          console.warn("视图快照保存失败（不影响统计）:", e);
        }
      }
    } finally {
      scheduled = false;
    }
  });
}

/** 测试钩子：等待队列排空（所有已入队 payload 派发完成）。 */
export async function snapshotQueueIdle(): Promise<void> {
  while (scheduled) {
    await tail;
  }
}

/** 测试钩子：最近一次派发错误（null = 无）。 */
export function lastSnapshotError(): unknown {
  return lastError;
}

/** 测试钩子：重置队列状态（测试隔离）。 */
export function resetSnapshotQueueForTests(): void {
  latest = null;
  scheduled = false;
  tail = Promise.resolve();
  lastError = null;
}
