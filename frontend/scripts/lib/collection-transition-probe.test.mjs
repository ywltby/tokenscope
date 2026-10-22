// @vitest-environment node
import { Window } from "happy-dom";
import { expect, it } from "vitest";
import { observeCollectionTransitions } from "./collection-transition-probe.mjs";

it("用受控时钟记录属性切换、元素重建和移除，不按轮询量化", async () => {
  const window = new Window();
  const { document } = window;
  let time = 0;
  const pill = () => document.querySelector(".ts-status-pill");
  const render = (state, runId) => {
    document.body.innerHTML = `<span class="ts-status-pill" data-collection-state="${state}" data-collection-run-id="${runId}"></span>`;
  };
  render("confirmed", 1);
  const probe = observeCollectionTransitions(document, () => time, window.MutationObserver);
  time = 100.25;
  pill().setAttribute("data-collection-state", "reading");
  pill().setAttribute("data-collection-run-id", "2");
  await window.happyDOM.whenAsyncComplete();
  time = 101.75;
  render("reading", 2);
  await window.happyDOM.whenAsyncComplete();
  time = 2601.5;
  pill().setAttribute("data-collection-state", "failed");
  await window.happyDOM.whenAsyncComplete();
  time = 2602.75;
  pill().remove();
  await window.happyDOM.whenAsyncComplete();
  time = 2700.5;
  render("reading", 3);
  await window.happyDOM.whenAsyncComplete();
  time = 2800.25;
  pill().setAttribute("data-collection-run-id", "4");
  await window.happyDOM.whenAsyncComplete();
  expect(probe.read()).toEqual([
    { at_ms: 0, state: "confirmed", runId: 1, initial: true },
    { at_ms: 100.25, state: "reading", runId: 2, initial: false },
    { at_ms: 101.75, state: "reading", runId: 2, initial: false },
    { at_ms: 2601.5, state: "failed", runId: 2, initial: false },
    { at_ms: 2602.75, state: null, runId: null, initial: false },
    { at_ms: 2700.5, state: "reading", runId: 3, initial: false },
    { at_ms: 2800.25, state: "reading", runId: 4, initial: false },
  ]);
  const copy = probe.read();
  copy.length = 0;
  expect(probe.read()).toHaveLength(7);
  probe.dispose();
  time = 3000;
  render("confirmed", 3);
  await window.happyDOM.whenAsyncComplete();
  expect(probe.read()).toHaveLength(7);
  await window.happyDOM.close();
});
