/// UX09：主题偏好的**无 Vue 依赖**解析（light | dark | system）。
///
/// 首帧前由 `public/theme-boot.js`（head 中的同源经典脚本，CSP `script-src
/// 'self'` 允许）消费**同一存储键与解析规则**；`composables/theme.ts` 复用
/// 这里的纯函数。两处规则一致性由 `preference_resolution_is_shared` 测试守住
/// ——避免首帧解析与运行时解析漂移（那正是"闪一下再切换"的成因）。
export type ThemePreference = "light" | "dark" | "system";
export type ThemeMode = "light" | "dark";

/** 主题偏好存储键（与 public/theme-boot.js 必须一致）。 */
export const THEME_STORAGE_KEY = "tokenscope-theme";

/** 合法偏好取值（其余一律回落 system）。 */
export const THEME_PREFERENCES: readonly ThemePreference[] = ["light", "dark", "system"];

/** 解析存储值：非 light/dark/system（含缺失、非法）一律回落 system。 */
export function parsePreference(raw: string | null | undefined): ThemePreference {
  return raw === "light" || raw === "dark" || raw === "system" ? raw : "system";
}

/** 偏好 + 系统明暗 → 解析后的实际主题。system 才跟随系统。 */
export function resolveMode(pref: ThemePreference, systemDark: boolean): ThemeMode {
  return pref === "system" ? (systemDark ? "dark" : "light") : pref;
}

/** 读取存储偏好；localStorage 不可用/抛错（隐私模式等）时回落 system。 */
export function readStoredPreference(): ThemePreference {
  try {
    return parsePreference(localStorage.getItem(THEME_STORAGE_KEY));
  } catch {
    return "system";
  }
}

/** 系统是否偏好深色；matchMedia 不可用时视为浅色（不抛错）。 */
export function systemPrefersDark(): boolean {
  try {
    return window.matchMedia("(prefers-color-scheme: dark)").matches;
  } catch {
    return false;
  }
}

/** 首帧前解析并落到 html[data-theme]（供 boot 脚本与运行时共用同一规则）。 */
export function applyResolvedThemeAttribute(): ThemeMode {
  const mode = resolveMode(readStoredPreference(), systemPrefersDark());
  document.documentElement.setAttribute("data-theme", mode);
  return mode;
}
