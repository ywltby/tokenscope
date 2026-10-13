// U03：设置项的串行保存队列（时区与 U06 的读取间隔共用同一机制）。
//
// 语义（计划 U03）：
// - 同一时刻最多一个保存请求在途；
// - 在途期间的新选择只记为「待保存值」（只保留最新）；
// - 在途请求结束后，如果待保存值与已落盘值不同，再发一次；
// - 界面显示用户最新的选择（不回退到在途值）；
// - 全部保存失败且没有待保存值时，回滚到最后一次成功落盘的值并暴露局部错误。
import { ref, type Ref } from "vue";

/** 保存函数：返回落盘后的值（undefined/null 视为入队值）；抛错 = 保存失败。 */
export type SettingSaver<T> = (value: T) => Promise<T | void | null>;

export interface SettingQueue<T> {
  /** 界面显示值 = 用户最后一次选择（不回退到在途值）。 */
  readonly value: Ref<T>;
  /** 是否有保存请求在途（用于显示「保存中…」）。 */
  readonly saving: Ref<boolean>;
  /** 最近一次保存失败的错误文本（后续保存成功后清空）。 */
  readonly error: Ref<string | null>;
  /** 提交一次新选择：立即更新显示值，并按串行规则保存。 */
  set(value: T): void;
  /** 外部权威值（启动读取/迁移基线）：记为已落盘值，不发保存。 */
  adopt(value: T): void;
  /** 等待队列排空（含失败回滚已完成）。 */
  idle(): Promise<void>;
  /** 测试钩子：把队列重置为空闲初态（不得在请求在途时调用）。 */
  resetForTests(persisted: T): void;
}

function errorTextOf(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/**
 * 创建串行保存队列。`persisted` 是启动时已知的已落盘值（时区用默认值，
 * 读取完成后由 `adopt` 更新）；`equals` 缺省用 `Object.is`。
 */
export function createSettingQueue<T>(opts: {
  save: SettingSaver<T>;
  persisted: T;
  equals?: (a: T, b: T) => boolean;
}): SettingQueue<T> {
  const equals = opts.equals ?? ((a: T, b: T) => Object.is(a, b));
  const value = ref(opts.persisted) as Ref<T>;
  const saving = ref(false);
  const error = ref<string | null>(null);

  /** 最后一次成功落盘的值（失败回滚的落点）。 */
  let persisted = opts.persisted;
  /** 待保存值：只保留最新一个（覆盖式）。 */
  let pending: { v: T } | null = null;
  let running = false;
  let waiters: (() => void)[] = [];

  function notifyIdle(): void {
    const list = waiters;
    waiters = [];
    for (const w of list) w();
  }

  function set(next: T): void {
    // 界面立即显示用户最新选择——不等保存结果，也不回退到在途值。
    value.value = next;
    pending = { v: next };
    void drain();
  }

  function adopt(next: T): void {
    persisted = next;
    value.value = next;
    error.value = null;
  }

  async function drain(): Promise<void> {
    if (running) return;
    running = true;
    saving.value = true;
    try {
      while (pending) {
        const target = pending.v;
        pending = null;
        if (equals(target, persisted)) {
          error.value = null;
          continue;
        }
        let ok = false;
        try {
          const saved = await opts.save(target);
          persisted = (saved === undefined || saved === null ? target : saved) as T;
          error.value = null;
          ok = true;
        } catch (e) {
          error.value = errorTextOf(e);
        }
        // 全部保存失败且没有更新的待保存值：回滚到最后一次成功落盘的值。
        // 若期间用户又选了新值，则不回滚——继续保存那个最新值。
        if (!ok && !pending) value.value = persisted;
      }
    } finally {
      saving.value = false;
      running = false;
      notifyIdle();
    }
  }

  async function idle(): Promise<void> {
    while (running || pending) {
      await new Promise<void>((resolve) => waiters.push(resolve));
    }
  }

  function resetForTests(initial: T): void {
    persisted = initial;
    value.value = initial;
    pending = null;
    error.value = null;
  }

  return { value, saving, error, set, adopt, idle, resetForTests };
}
