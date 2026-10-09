import { onMounted, onUnmounted } from "vue";

/** 捕获原生滚动，也覆盖传送到 body 的浮层；每个容器独立计时。 */
export function useAutoHideScrollbars(): void {
  const timers = new Map<HTMLElement, ReturnType<typeof setTimeout>>();
  function onScroll(event: Event): void {
    const target = event.target === document ? document.scrollingElement : event.target;
    if (!(target instanceof HTMLElement) || target.scrollHeight <= target.clientHeight) return;
    clearTimeout(timers.get(target));
    target.classList.add("ts-scrolling");
    timers.set(
      target,
      setTimeout(() => {
        target.classList.remove("ts-scrolling");
        timers.delete(target);
      }, 800),
    );
  }
  onMounted(() => document.addEventListener("scroll", onScroll, { capture: true, passive: true }));
  onUnmounted(() => {
    document.removeEventListener("scroll", onScroll, true);
    for (const [target, timer] of timers) {
      clearTimeout(timer);
      target.classList.remove("ts-scrolling");
    }
    timers.clear();
  });
}
