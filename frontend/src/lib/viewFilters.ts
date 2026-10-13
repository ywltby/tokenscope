// U04（UI 调整计划 2026-10-13）：筛选偏好——本机视图便利，与主题、颜色同类。
//
// 偏好决定「查什么」：来源、聚合维度、时间预设、日粒度参与方式、下钻。
// 与视图快照的分工：
// - 偏好是查询口径的唯一来源，启动时同步读取，变化即写盘；
// - 视图快照只在「偏好 + 时区」逐项相等时用于首帧展示（并标记 stale），
//   从不反向改写偏好，也不写时区（时区的唯一来源是 settings.toml）。
//
// 非法值一律**逐字段**回退默认（全部来源 / 按时间 / 全部时间 / 自动参与 /
// 无下钻）——脏值绝不能带进 query_begin 的参数。
import type { AgentFilter, Dim, EventDrill } from "../types";
import { readItem, writeItem } from "./localStorage";
import { normalizePreset, presetKey, type RangePreset } from "./rangePreset";

/** H08：日粒度历史是否参与统计（`detail_only` = 只看请求明细）。 */
export type RollupsMode = "auto" | "detail_only";

export interface ViewFilters {
  by: Dim;
  agent: AgentFilter;
  preset: RangePreset | null;
  rollupsMode: RollupsMode;
  drill: EventDrill | null;
}

export const VIEW_FILTERS_KEY = "tokenscope-view-filters";

export const DEFAULT_VIEW_FILTERS: ViewFilters = {
  by: "day",
  agent: "all",
  preset: null,
  rollupsMode: "auto",
  drill: null,
};

const DIMS: readonly Dim[] = ["day", "model", "project", "agent"];
const AGENTS: readonly AgentFilter[] = ["all", "claude", "codex"];
const DRILL_TYPES: readonly EventDrill["type"][] = ["day", "model", "project"];

/** `{kind:"all"}` 与 `null` 都是「全部时间」——统一存成 null（稳定身份）。 */
function canonicalPreset(preset: RangePreset | null | undefined): RangePreset | null {
  return !preset || preset.kind === "all" ? null : preset;
}

/** 下钻白名单：类型必须是 day/model/project，键必须是非空字符串。 */
export function normalizeDrill(value: unknown): EventDrill | null {
  if (value == null || typeof value !== "object") return null;
  const v = value as Record<string, unknown>;
  if (!DRILL_TYPES.includes(v.type as EventDrill["type"])) return null;
  if (typeof v.key !== "string" || v.key === "") return null;
  const drill: EventDrill = { type: v.type as EventDrill["type"], key: v.key };
  if (typeof v.label === "string" && v.label !== "") drill.label = v.label;
  return drill;
}

/** 逐字段校验并归一化；任何非法字段回退默认值。 */
export function normalizeViewFilters(value: unknown): ViewFilters {
  if (value == null || typeof value !== "object") return { ...DEFAULT_VIEW_FILTERS };
  const v = value as Record<string, unknown>;
  return {
    by: DIMS.includes(v.by as Dim) ? (v.by as Dim) : DEFAULT_VIEW_FILTERS.by,
    agent: AGENTS.includes(v.agent as AgentFilter)
      ? (v.agent as AgentFilter)
      : DEFAULT_VIEW_FILTERS.agent,
    preset: canonicalPreset(normalizePreset(v.preset)),
    // 除显式 `detail_only` 外一律 auto（与快照恢复同一口径）。
    rollupsMode: v.rollupsMode === "detail_only" ? "detail_only" : "auto",
    drill: normalizeDrill(v.drill),
  };
}

/** 启动同步读取：缺失、JSON 损坏、存储不可用都回默认。 */
export function loadViewFilters(onError?: (e: unknown) => void): ViewFilters {
  const raw = readItem(VIEW_FILTERS_KEY, onError);
  if (raw == null) return { ...DEFAULT_VIEW_FILTERS };
  try {
    return normalizeViewFilters(JSON.parse(raw));
  } catch {
    return { ...DEFAULT_VIEW_FILTERS };
  }
}

/** 写入（存储不可用只降级不抛错，返回是否落盘）。 */
export function saveViewFilters(filters: ViewFilters): boolean {
  const payload: ViewFilters = { ...filters, preset: canonicalPreset(filters.preset) };
  return writeItem(VIEW_FILTERS_KEY, JSON.stringify(payload));
}

/** 逐项相等（预设按身份键、下钻按类型与键；label 只是文案，不参与）。 */
export function viewFiltersEqual(a: ViewFilters, b: ViewFilters): boolean {
  return (
    a.by === b.by &&
    a.agent === b.agent &&
    a.rollupsMode === b.rollupsMode &&
    presetKey(a.preset) === presetKey(b.preset) &&
    (a.drill?.type ?? null) === (b.drill?.type ?? null) &&
    (a.drill?.key ?? null) === (b.drill?.key ?? null)
  );
}
