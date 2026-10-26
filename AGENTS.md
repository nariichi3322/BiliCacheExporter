## Branches and releases

- 不直接提交或推送到 `main`。在其他分支開發，完成後開 PR 合併進 `main`（私有 repo 無法強制分支保護，靠此約定）。
- `.github/workflows/ci.yml` 對每個 PR 與 `main` 更新執行 `check.ps1`。
- 發版：在分支上把 `desktop/src-tauri/tauri.conf.json`、`desktop/package.json`、`desktop/src-tauri/Cargo.toml` 的版本號一起提高 → 本機執行 `desktop/package.ps1` 完成完整驗證（CI 不跑 GUI 驗證）→ 開 PR，內文即 release 說明 → 合併後 CI 自動建立 `v<版本>` tag 與正式 release。

## Agent skills

### Issue tracker

規格與議題使用本機 Markdown。建立、讀取或更新前，先讀取 docs/agents/issue-tracker.md。

### Triage labels

使用五種預設狀態。分類或變更狀態前，先讀取 docs/agents/triage-labels.md。

### Domain docs

採 single-context。探索專案前，依 docs/agents/domain.md 讀取相關領域文件。
