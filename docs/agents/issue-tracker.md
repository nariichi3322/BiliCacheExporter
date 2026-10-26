# Issue tracker: GitHub

規格與議題使用本專案的 GitHub Issues，透過 gh CLI 操作。
從 git remote 判定儲存庫。

## 操作

- 建立：gh issue create --title "..." --body-file <file>
- 讀取：gh issue view <number> --json number,title,body,labels,comments
- 列表：gh issue list --state open --json number,title,body,labels
- 留言：gh issue comment <number> --body-file <file>
- 標籤：gh issue edit <number> --add-label "..." 或 --remove-label "..."
- 完成：gh issue close <number> --reason completed
- 不處理：套用 wontfix，留言說明後以 --reason "not planned" 關閉。

多行內容寫入暫存檔，再使用 --body-file。
分流標籤名稱見 triage-labels.md。

## 完成與依賴

- 分流使用標籤；完成狀態使用 Issue 的開啟／關閉狀態。
- 完成時勾選已驗證的驗收條件，留言記錄提交與測試證據，再關閉。
- 等待實機驗收的議題保留開啟與未完成條件，不以模擬測試代替。
- 使用 GitHub 原生子議題與依賴關係；無法使用時，在本文記錄
  Part of #<parent> 與 Blocked by: #<number>。
- 解除依賴前確認阻擋議題已完成；以「不處理」關閉時，重新評估依賴。

## 技能用語

技能要求「發布議題」時，建立或更新 GitHub Issue。
技能要求「取得議題」時，讀取指定編號的 GitHub Issue。

## Pull requests as a triage surface

**PRs as a request surface: no.**

## 既有本機資料

.scratch/ 中的既有資料保留。本次設定不搬移、不刪除；
明確指定本機檔案時仍可讀取，新議題使用 GitHub Issues。
