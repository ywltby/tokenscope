// P04：首帧主题引导脚本（head 中的**同源经典脚本**，同步执行、阻塞解析，
// 因此早于首次绘制；生产 CSP script-src 'self' 允许，不能用内联脚本）。
//
// 未同意隐私政策之前**不读取持久化偏好**（WebView 的持久化存储不参与同意
// 前的首屏路径）：首帧只跟随系统明暗，写入 data-theme；用户保存的主题偏好
// 在后端 Ready 之后、业务组件挂载之前由 `src/lib/themePreference.ts` 的
// applyResolvedThemeAttribute() 显式恢复。
//
// 失败降级：任何异常都不阻断页面，直接按浅色渲染（tokens.css 的 :root 默认）。
(function () {
  var mode = "light";
  try {
    mode = window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  } catch (e) {
    mode = "light";
  }
  document.documentElement.setAttribute("data-theme", mode);
})();
