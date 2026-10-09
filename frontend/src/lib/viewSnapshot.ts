// R04（全计划审核 Task 4）：视图快照保存队列。
// 模块级——App 用 v-if 切换汇总/设置页，Dashboard 卸载不能丢保存顺序；
// 合并待保存最新项 + 串行派发：绝不并发 fire-and-forget 让旧写覆盖新写。
import { invoke } from "@tauri-apps/api/core";

/** 快照格式版本：v8 = 项目根归并（B02/B03）——分组与下钻 key 改为「项目根 +
 * 子目录归并、越界成新项目」口径，v7 及更早快照里的 key 均已失效，读取时
 * 必须忽略（否则启动瞬间会闪现旧分组）；v7 = 统一项目身份（阶段 A）；
 * v6 = 查询会话契约（SF04）；v5 及更早版本不含会话身份且游标为旧格式。 */
export const SNAPSHOT_VERSION = 8;

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
