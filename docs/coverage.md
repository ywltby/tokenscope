# 本地代码覆盖率（三语言独立口径）

本文记录 TokenScope 本地覆盖率工具的安装、复现命令与统计口径。**只服务本地
取证：现有 CI 门禁保持不变，不设覆盖率阈值，报告不入库**（全部落在
`qa-artifacts/coverage/`，该目录已被 gitignore）。三种语言的统计工具、范围与
口径互不相同，**不得硬合成为一个"全项目覆盖率"数字**。

## 环境（基线取证时实测）

| 工具 | 版本 |
| --- | --- |
| Rust 工具链 | stable-x86_64-pc-windows-msvc（cargo 1.99.0） |
| cargo-llvm-cov | 0.9.1（`cargo install cargo-llvm-cov --locked --version 0.9.1`） |
| llvm-tools-preview | 随工具链（`rustup component add llvm-tools-preview`） |
| Node / pnpm | v24.14.0 / 11.22.0 |
| Vitest / @vitest/coverage-v8 | 5.0.3 / 5.0.3（精确匹配，devDependencies 固定） |
| Python | 3.14.3 |
| coverage.py | 7.17.0（`requirements-dev.txt` 固定） |

## 前端（Vitest + v8）

- 范围：`frontend/src/**/*.{ts,vue}` 全部产品源码；`all: true` 使**未被任何
  测试导入的文件也按 0% 计入**（如基线中的 `src/dev/tooltipSecurityPage.ts`）。
  排除测试本体（`src/**/*.test.ts`）与纯类型声明（`src/**/*.d.ts`，防御性，
  当前 src 下没有）。`scripts/lib/**/*.test.mjs` 属原生验收契约测试，不在
  统计范围（其被测对象是 node 脚本，非前端产品源码）。
- 运行（独立命令；普通 `pnpm test` 行为不变，仍为 `vitest --run`，基线
  64 文件 / 912 用例通过且不生成报告）：

  ```bash
  pnpm --dir frontend run test:coverage
  ```

- 报告：文本汇总打到 stdout；HTML / lcov.info / json-summary 落
  `qa-artifacts/coverage/frontend-vitest/`（目录由 `vitest.config.ts` 的
  `coverage.reportsDirectory` 指定）。

## Rust（cargo-llvm-cov，本地运行；不进 CI）

前置（一次性）：

```bash
rustup component add llvm-tools-preview
cargo install cargo-llvm-cov --locked --version 0.9.1
```

**已验证的运行方式（工具自身编排 + 复用测试包装器的隔离机制）**：
`cargo llvm-cov` 直接编排插桩构建、测试与报告；临时根隔离与退出后清理核验
复用 `scripts/run_rust_tests.py` 的 `run()`（其 CLI 强制 `cargo test` 参数，
故不经 CLI 传 llvm-cov，而是程序化调用同一 `run()`——同一次性临时根
（TMP/TEMP/TMPDIR）与清理核验语义）。注意：cargo-llvm-cov 0.9.1 的
`show-env` + 外部 `cargo test` 包装器流程在本机（cargo 1.99.0，MSVC）实测
**不产生插桩产物**（测试二进制无 `__llvm_prf*` 符号、profraw 为零，已诊断
并放弃），不要沿用该流程。

```bash
# 1) 采集（--no-report：构建+运行全部 workspace 测试，保留 profraw）
python -B -c "import sys; sys.path.insert(0,'scripts'); from run_rust_tests import run; sys.exit(run(['cargo','llvm-cov','--locked','--workspace','--no-report'], cwd='.'))"
# 2) 报告（同一目录产物：全表文本 / json / html / lcov）
cargo llvm-cov report
cargo llvm-cov report --json --output-path qa-artifacts/coverage/rust-llvm-cov/normal-coverage.json
cargo llvm-cov report --html --output-dir qa-artifacts/coverage/rust-llvm-cov/html
cargo llvm-cov report --lcov --output-path qa-artifacts/coverage/rust-llvm-cov/lcov.info
```

- 隔离：在独立 worktree 运行（本仓库按会话使用 worktree，`target/` 天然按
  worktree 分离，不影响其他会话构建产物）；`CARGO_BUILD_JOBS=2` 规避本机
  链接器 OOM（环境变量，不属包装器）；被 `#[ignore]` 的真实数据性能测试
  （`perf_real_data`、`perf_history_real`）不会被普通 `cargo test` 执行。
- 口径：0.9.1 输出**行（Lines）/ 区（Regions）/ 函数（Functions）**三项；
  **Branch 指标在该工具链输出为 `-`（不提供）**，如实记录、不虚报。
- 普通模式与 acceptance 模式分开采集、分开报告；两次运行之间先
  `cargo llvm-cov clean`（清 profraw/profdata，保留插桩构建产物）：

  ```bash
  python -B -c "import sys; sys.path.insert(0,'scripts'); from run_rust_tests import run; sys.exit(run(['cargo','llvm-cov','clean'], cwd='.'))"
  python -B -c "import sys; sys.path.insert(0,'scripts'); from run_rust_tests import run; sys.exit(run(['cargo','llvm-cov','--locked','--workspace','--features','acceptance','--no-report'], cwd='.'))"
  ```

  acceptance 模式只跑普通 cargo 测试（CI rust-acceptance 同源），**不含
  Native 场景**。

## Python（coverage.py，scripts/ 测试套件）

环境（一次性，隔离于仓库内 `.venv/`，已 gitignore；版本由
`requirements-dev.txt` 固定）：

```bash
python -m venv .venv
./.venv/Scripts/python -m pip install -r requirements-dev.txt   # coverage==7.17.0
```

运行（与提交钩子同一套用例：`unittest discover -s scripts -p "test_*.py"`）：

```bash
./.venv/Scripts/python -m coverage run --source=scripts -m unittest discover -s scripts -p "test_*.py"
./.venv/Scripts/python -m coverage report --show-missing
./.venv/Scripts/python -m coverage json -o qa-artifacts/coverage/python-scripts/coverage.json
./.venv/Scripts/python -m coverage html -d qa-artifacts/coverage/python-scripts/html
```

- 范围：`--source=scripts` 统计 `scripts/*.py`（含测试文件本身——它们是
  套件的一部分，报告中可见其自覆盖率）。
- **子进程不纳入统计**：默认 coverage.py 不采集子进程；本套件确有测试以
  `sys.executable -B scripts/...` 方式调用 `inspect_history.py`、
  `make_ccs_fixture.py` 等（这些调用路径未被采集，相关脚本覆盖率因此是
  **下界**）。未启用 `COVERAGE_PROCESS_START` + `.pth` 子进程钩子；未采集
  部分不得解读为"未覆盖即未测"，也不虚报为已覆盖。

## 报告位置

`qa-artifacts/coverage/{frontend-vitest,rust-llvm-cov,python-scripts}/`
（全部 gitignore，不入库）。当次基线的受测提交、工具版本、口径、排除项与
跳过项记录在同目录 `BASELINE-README.txt`。
