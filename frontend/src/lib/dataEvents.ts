/// 设置页写入数据后广播的统一事件名（价格同步、来源保存、重扫、导入）。
///
/// 汇总页由 KeepAlive 保留，停用期间 `refreshKey` prop 不保证更新；这个
/// 文档级事件让隐藏中的查询状态机也能记下"下一批必须先采集"，避免复用
/// 同步前的价格/来源快照。事件不携带数据，只表示"已提交写入"；`detail`
/// 是同一次写入的 refreshKey 值，让查询侧识别"同一次变更"的两个信号
/// （事件先到、prop 随渲染后到）并合并为一批。
export const DATA_CHANGED_EVENT = "tokenscope-data-changed";

/// 广播一次数据变更。调用方是 MainApp 的 `data-changed` 处理器；`token`
/// 是本次写入递增后的 refreshKey 值（未提供 = 未知来源的独立变更）。
export function emitDataChanged(token?: number): void {
  if (typeof window !== "undefined") {
    window.dispatchEvent(new CustomEvent(DATA_CHANGED_EVENT, { detail: token }));
  }
}
