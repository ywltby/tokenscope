// @vitest/coverage-v8 的未执行文件补算只在文件完全未出现在覆盖表中时生效。
// 本项目的契约测试用 `import.meta.glob(..., { query: "?raw", eager: true })`
// 把全部源码以原始字符串模块导入；V8 把这些空壳模块映射回源码路径，
// 于是未执行文件以 0/0 出现在覆盖表里，补算被跳过。这里在汇总后把没有
// 语句的空壳条目当作未执行文件重新补算，使它们以 0/N 计入分母。
import { V8CoverageProvider } from "@vitest/coverage-v8/dist/provider.js";
import type { ReportContext } from "vitest/node";

type CoverageMap = Awaited<ReturnType<V8CoverageProvider["generateCoverage"]>>;

type UncoveredFileCounter = {
  getCoverageMapForUncoveredFiles(testedFiles: string[]): Promise<CoverageMap>;
};

class RawModuleAwareV8Provider extends V8CoverageProvider {
  override async generateCoverage(context: ReportContext): Promise<CoverageMap> {
    const coverageMap = await super.generateCoverage(context);
    const coveredFiles = coverageMap.files().filter((file) => {
      const data = coverageMap.fileCoverageFor(file).data;
      return Object.keys(data.statementMap).length > 0;
    });
    const uncovered = await (
      this as unknown as UncoveredFileCounter
    ).getCoverageMapForUncoveredFiles(coveredFiles);
    coverageMap.merge(uncovered);
    return coverageMap;
  }
}

export default {
  getProvider() {
    return new RawModuleAwareV8Provider();
  },
};
