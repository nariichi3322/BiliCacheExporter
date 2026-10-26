## Branches and releases

- 在其他分支開發，完成後開 PR 合併進 `main`；pre-commit hook 拒絕在 `main` 上提交。
- `.github/workflows/ci.yml` 對每個 PR 與 `main` 更新執行 `check.ps1`。
- `check.ps1` 不跑介面整合流程；改到畫面文字、按鈕或流程後，照 README〈從原始碼建置〉的快速 e2e 指令重跑。
- 發版：在分支上把 `tauri.conf.json`、`package.json`、`package-lock.json`、`Cargo.toml` 的版本號一起提高（CI 檢查一致）→ 本機執行 `desktop/package.ps1` 完成完整驗證（CI 不跑 GUI 驗證）→ 開 PR，內文加 `## Release notes` 段落寫給使用者看的說明（沒有此段落時整份內文就是說明）→ 合併後 CI 自動建立 `v<版本>` tag 與正式 release。

## Agent skills

### Issue tracker

規格與議題使用 GitHub Issues。建立、讀取或更新前，先讀取 docs/agents/issue-tracker.md。

### Triage labels

使用五種預設標籤。分類或變更標籤前，先讀取 docs/agents/triage-labels.md。

### Domain docs

採 single-context。探索專案前，依 docs/agents/domain.md 讀取相關領域文件。
