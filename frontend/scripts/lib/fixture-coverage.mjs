// AC08：fixture 覆盖表聚合——覆盖必须来自当次运行产物（巡检行与契约
// 实际打开记录），不能用源码名单或 required 清单冒充。
//
// 输入：
// - registered：全部注册 fixture 名（FIXTURE_NAMES）；
// - patrol：巡检运行行 [{ fixture, ok }]（ok 不影响覆盖判定——覆盖记录
//   运行事实，失败也是运行过）；
// - contracts：契约实际打开记录 [{ contract, fixtures: [name] }]（来自
//   runContracts 循环内生效的 ctx.open 记录点）。
// 输出：
// - rows：每个注册 fixture 一行 { fixture, patrol, contracts }（contracts
//   去重、保持首现顺序）；
// - uncovered：巡检与契约都未覆盖的注册 fixture（缺口，调用方上报）；
// - unregistered：运行产物中出现但未注册的 fixture（异常，调用方上报）。

/** @returns {{rows: {fixture: string, patrol: boolean, contracts: string[]}[], uncovered: string[], unregistered: string[]}} */
export function computeFixtureCoverage({ registered, patrol, contracts }) {
  const registeredSet = new Set(registered);
  const patrolRan = new Set(
    (patrol ?? []).map((row) => row.fixture).filter((name) => typeof name === "string"),
  );
  const contractFixtures = new Map();
  const unregistered = new Set();
  for (const entry of contracts ?? []) {
    for (const fixture of entry.fixtures ?? []) {
      if (typeof fixture !== "string" || fixture === "") continue;
      if (!registeredSet.has(fixture)) unregistered.add(fixture);
      if (!contractFixtures.has(fixture)) contractFixtures.set(fixture, []);
      const list = contractFixtures.get(fixture);
      if (!list.includes(entry.contract)) list.push(entry.contract);
    }
  }
  const rows = registered.map((fixture) => ({
    fixture,
    patrol: patrolRan.has(fixture),
    contracts: contractFixtures.get(fixture) ?? [],
  }));
  const uncovered = registered.filter(
    (fixture) => !patrolRan.has(fixture) && !contractFixtures.has(fixture),
  );
  return { rows, uncovered, unregistered: [...unregistered].sort() };
}
