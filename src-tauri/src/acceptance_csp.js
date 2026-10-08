// 仅 acceptance 二进制提供；普通页面事件创建 script，绝不使用 eval/CDP 求值注入。
(() => {
  const result = { started: false, positiveRan: false, inlineRan: false, violations: [] };
  window.__tsCspProbe = result;
  document.addEventListener("securitypolicyviolation", (event) => {
    if (event.effectiveDirective === "script-src-elem") {
      result.violations.push({ blockedURI: event.blockedURI, directive: event.effectiveDirective });
    }
  });
  const button = document.createElement("button");
  button.id = "ts-acceptance-csp";
  button.textContent = "运行 CSP 验收哨兵";
  button.style.cssText = "position:fixed;bottom:8px;left:8px;z-index:99999";
  button.addEventListener("click", () => {
    if (result.started) return;
    result.started = true;
    const positive = document.createElement("script");
    positive.src = "/__acceptance_positive.js";
    document.head.append(positive);
    const inline = document.createElement("script");
    inline.textContent = "window.__tsCspProbe.inlineRan = true;";
    document.head.append(inline);
    const external = document.createElement("script");
    external.src = "https://tokenscope.invalid/acceptance-blocked.js";
    document.head.append(external);
  });
  document.body.append(button);
})();
