# CI 与自动发布计划

## 已确认的产品行为

- 仅 `main` 普通提交生成公开预发布；版本 tag 检查通过后自动公开正式 Release
- 保留 Windows x64 NSIS 为当前生产目标
- 本次以独立分支、draft PR 交付，不合并、不创建 tag、不手动触发发布

## 实施方案（已确认）

1. 将现有根库、Tauri 壳、前端门禁复用给 CI 和 Release；保留默认 feature 检查，并增加 acceptance feature 检查
2. CI 服务 PR 与 main；Release 服务 main 提交与 `v*` tag，同一触发 SHA 通过门禁后再构建
3. main 预发布使用完整 commit SHA 唯一 tag；版本 tag 严格校验 SemVer 与应用版本，带预发布后缀的 tag 仍是预发布
4. 发布 Windows x64 NSIS、SHA-256 校验和与 GitHub Artifact Attestations；生产构建不启用 acceptance
5. 发布权限只授予对应 job；工作流 Action 固定到完整 SHA、依赖锁定安装、合理缓存/超时；CI 按 PR/ref、Release 按 main/各 tag 分组取消旧运行
6. 增加发布元数据与边界测试、操作说明；静态验证并通过 draft PR CI 验证，明确未执行的真实 Windows 打包/发布步骤

## 已批准的权限

发布 job 需要 `contents: write` 创建 Release/tag、上传附件；证明 job 需要 `id-token: write` 与 `attestations: write` 生成短期 OIDC 构建来源证明。不会读取或新增仓库密钥，也不会给 PR 门禁写权限。
