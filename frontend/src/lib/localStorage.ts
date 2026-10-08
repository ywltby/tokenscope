/// localStorage 安全读写（RC09：storage 不可用不得阻断应用）。
///
/// WebView2 / 浏览器隐私模式、被策略禁用本地存储或配额耗尽时，访问
/// `localStorage` 会**直接抛错**（连 getter 都取不到）。这类失败只意味着
/// "偏好无法持久化"，不是应用故障：读回落默认值、写静默放弃，绝不把
/// 组件初始化打断（此前 theme/timezone 的 `watchEffect` 里裸调 setItem，
/// 存储不可用时整个 setup 抛错，窗口停在空白）。
///
/// 失败通过 `onStorageError` 回调暴露给调用方（默认无操作），便于测试断言
/// "确实降级过"，不在生产路径上刷屏日志。

/** 读取；不可用/抛错/缺失都返回 null（由调用方决定默认值）。 */
export function readItem(key: string, onError?: (e: unknown) => void): string | null {
  try {
    return window.localStorage.getItem(key);
  } catch (e) {
    onError?.(e);
    return null;
  }
}

/** 写入；不可用或配额失败返回 false，绝不抛错。 */
export function writeItem(key: string, value: string, onError?: (e: unknown) => void): boolean {
  try {
    window.localStorage.setItem(key, value);
    return true;
  } catch (e) {
    onError?.(e);
    return false;
  }
}
