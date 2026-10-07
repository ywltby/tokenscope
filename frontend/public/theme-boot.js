// UX09：首帧前主题引导脚本（head 中的**同源经典脚本**，同步执行、阻塞解析，
// 因此早于首次绘制；生产 CSP script-src 'self' 允许，不能用内联脚本）。
//
// 规则必须与 src/lib/themePreference.ts 一致：存储键 tokenscope-theme、
// 取值 light|dark|system、非法/缺失/存储不可用一律回落 system、只有 system
// 才跟随系统明暗。一致性由 preference_resolution_is_shared 测试守住。
//
// 失败降级：任何异常都不阻断页面，直接按浅色渲染（tokens.css 的 :root 默认）。
(function () {
  var KEY = "tokenscope-theme";
  var mode = "light";
  try {
    var raw = null;
    try {
      raw = window.localStorage.getItem(KEY);
    } catch (e) {
      raw = null;
    }
    var pref = raw === "light" || raw === "dark" || raw === "system" ? raw : "system";
    var dark = pref === "dark";
    if (pref === "system") {
      try {
        dark = window.matchMedia("(prefers-color-scheme: dark)").matches;
      } catch (e) {
        dark = false;
      }
    }
    mode = dark ? "dark" : "light";
  } catch (e) {
    mode = "light";
  }
  document.documentElement.setAttribute("data-theme", mode);
})();
