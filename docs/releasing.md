# CI 与发布

## 触发与取消

- `CI`：PR 和 `main` push，根库/壳默认 feature 与 acceptance feature 各自 clippy/test，加 fmt、前端类型/格式/测试/构建
- `Release`：`main` push 与 `v*` tag；重新检查同一 SHA 后构建 Windows x64 NSIS
- `main` 产出公开预发布 `pre-<完整提交 SHA>`，不会被标为 latest，也不会因生成 tag 递归触发
- `vX.Y.Z` 是正式版；`vX.Y.Z-rc.N` 等仍是预发布。tag 必须是 main 历史中的提交，且版本与两个 Cargo.toml、frontend/package.json、src-tauri/tauri.conf.json 完全一致
- 两个工作流互不取消。CI 按 PR/ref、Release 按 ref 分组，新运行取消同组旧运行，不同版本 tag 互不取消。因此快速连续 push 可能只有最新 main 提交完成发布；已公开的旧 Release 保留

## 准备正式版

1. 在 PR 中同步四处版本，并更新对应 Cargo.lock 中本项目版本；完整检查通过后按仓库流程合并
2. 在所需 main 提交上创建并推送 `vX.Y.Z` tag；不要重用/移动已发布 tag
3. 查看 Release 工作流；门禁、生产打包与来源证明均成功后自动公开 Release

生产打包使用默认 feature，不启用 acceptance。CI 的 acceptance feature 测试不等于原生 GUI 端到端人工验收；现有原生验收流程仍需按发布要求执行。

## 附件与验证

每个 Release 附带 Windows x64 NSIS `.exe`、`SHA256SUMS.txt` 与 `provenance.sigstore.json`。在下载目录执行：

```powershell
Get-FileHash .\TokenScope_<tag>_windows-x64-setup.exe -Algorithm SHA256
# 与 SHA256SUMS.txt 对比
gh attestation verify .\TokenScope_<tag>_windows-x64-setup.exe --repo ywltby/tokenscope
```

GitHub 来源证明证明构建来源，不是 Windows Authenticode 代码签名，也不消除 SmartScreen 警告。

## 权限、安全与恢复

- 默认 `contents: read`；构建证明 job 增加 `id-token: write`、`attestations: write`；最终发布 job 仅 `contents: write`
- 采用 GitHub `actions/attest`，禁用容器存储记录，不需要 `artifact-metadata: write`、PAT 或新增仓库密钥
- 使用完整 action SHA、冻结 pnpm/Cargo 锁文件、非持久 checkout 凭据、独立超时与缓存
- 附件先构建、校验、证明，随后创建草稿、上传齐全，最后公开；失败/被取消的发布可能留下草稿。同 SHA 重跑可以补齐草稿；公开版本不覆盖附件
- 同名 Release/tag 必须对应相同 SHA；禁止将已有 Release 指向另一提交
- 没有 `workflow_dispatch` 发布入口，本 PR 不创建 tag、不公开任何 Release。合并后 main push 才开始正式执行预发布流程
