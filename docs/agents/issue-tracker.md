# Issue tracker: Local Markdown

規格與議題存於本機 .scratch/，路徑以專案根目錄為準。

- 每個功能一個目錄：.scratch/<feature-slug>/
- 規格：.scratch/<feature-slug>/spec.md
- 工作項目：.scratch/<feature-slug>/issues/<NN>-<slug>.md
- 工作項目從 01 編號，每個項目獨立一檔。
- 規格與工作項目頂端以 Status: 記錄狀態，名稱見 triage-labels.md。
- 討論附加於檔案底部的 ## Comments。

## 完成與依賴

- Status 是分流標籤；完成狀態另以 Resolution: resolved 記錄，省略或 open 表示尚未完成。
- 完成時勾選已驗證的驗收條件，於 Comments 記錄提交與測試證據，再標記 resolved。
- 只有已 resolved 的工作票能解除依賴。等待實機驗收的票保留未完成條件，不以模擬測試代替。

技能要求「發布議題」時，建立或更新上述本機檔案。
技能要求「取得議題」時，讀取指定路徑；只有編號時，
在對應功能的 issues 目錄尋找。
