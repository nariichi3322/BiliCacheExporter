# 01: 新版啟動、設定與深淺色

Status: ready-for-agent

Resolution: resolved

**What to build:** 可啟動的 Tauri／Vue 3／TypeScript／Rust 桌面應用，呈現使用者選定的精簡清單布局，能保存主題、手動工具路徑與輸出／暫存設定。與舊版並存，後續票逐步接入真實功能，不要求先重構舊 C#。

**Blocked by:** None (can start immediately).

規格：[桌面介面與可攜發行改版](../spec.md)。

- [x] Windows 可啟動新版；介面為頂部連線、中央空清單、底部輸出／匯出，設定另頁、記錄可收合。沒有裝置資料時不展示原型假資料；未實作操作不假裝成功。
- [x] 預設深色與藍色主色；淺色模式涵蓋所有目前可見控制項，切換後重啟保留。鍵盤與窄視窗下設定及返回可用。
- [x] 從介面指定與保存 ADB、FFmpeg／FFprobe、輸出與暫存位置，透過 Rust 驗證工具可啟動；不依賴 PATH，缺少或錯誤工具有可操作提示。
- [x] 設定留在可攜資料夾；讀取舊版有效設定，不破壞原檔、不默默丟棄值；損壞設定與不可寫位置清楚報錯，不靜默改到其他位置。
- [x] 建立後續操作所需的最少應用命令及可替換外部工具執行邊界；使用結構化引數，不向前端暴露任意 shell 執行能力。解析保留原始輸出，日誌寫入時才遮蔽敏感資料。
- [x] 使用者可查看基本診斷記錄，記錄留在 log 資料夾，不刪舊日誌、不自動上傳；遮蔽裝置序號、URL 查詢參數及個人路徑。
- [x] 新版與既有版本各自可建置，不刪除舊來源、歷史驗收或發行成品；提供可重現開發指令，新增必要的型別／建置／測試檢查，不繞過既有提交檢查。
- [x] 以應用操作驗證設定升級、保存重啟、工具錯誤及日誌遮蔽；不需要真手機，啟動與待機不觸發裝置查詢。

## Comments

- 2026-10-26：依使用者確認發布。只交付本票可用流程，不在本票完成所有核心移植。
- 2026-10-26：完成並驗證（提交 dfb7e17，規格與工作票同在此提交）。新版位於 `desktop/`（Tauri 2.12、Vue 3.5、TypeScript 5.9、Rust 1.99），舊 C# 原始碼、歷史驗收與 dist 成品未刪除；`check.ps1` 在原有 C# 檢查之後加跑 `desktop/check.ps1`（vue-tsc、Vitest、cargo fmt、clippy -D warnings、cargo test），兩次提交都經 pre-commit hook 全數通過。
  - 應用層命令：`App`（`src-tauri/src/app.rs`）提供 snapshot／save_settings／reset_settings／check_tools／connect／scan／export／cancel／recover_jobs／download_tools；前端只能呼叫這些命令，`open_folder` 只接受 log／output 兩種。外部程序一律經 `Runner`（結構化引數、無 shell），測試以 `Runner::with_fake` 替換 ADB；解析用原始輸出，寫入日誌時才 `redact`。
  - Rust 流程測試（`tests/flows.rs`，真實 FFmpeg 9.0.1）：`v1_settings_upgrade_keeps_values_and_original`（舊鍵值、未知欄位保留、首次升級留 `settings.v1-backup.json`、主題重啟保留）、`corrupt_settings_are_reported_not_replaced`、`unwritable_folder_blocks_with_actionable_message`、`tools_are_validated_by_running_them_not_by_path`（實際執行工具、不靠 PATH、缺少時回傳可下載項目）、`startup_and_idle_never_query_devices`、`logs_go_to_log_folder_redacted_and_keep_old_logs`。
  - 正式 EXE 端到端（`desktop/e2e/package-e2e.mjs`，假手機）：啟動與 3 秒閒置 0 次 ADB 呼叫；預設深色；無原型假資料；淺色切換、設定頁、記錄展開、420px 寬視窗無水平捲動、重啟主題保留、程式資料夾不可寫時顯示移至可寫位置提示。截圖見 08 的 evidence。
  - Vitest（`src/App.test.ts`）：啟動只呼叫 snapshot／ready；主題保存；設定頁返回；損壞設定顯示「備份並重設」並停用操作。
  - 補充：正式 EXE 端到端以 Tab＋Enter 開啟設定並返回清單；真實視窗縮到允許的最小尺寸（420×480 邏輯像素，本機 DPI 200%）版面完整。
- 2026-10-26：08 打包驗證時發現並修正（提交 6099e16）：設定頁開啟時的工具檢查若比「儲存設定」後的檢查晚完成，會用舊結果覆蓋新結果。改為只採用最新一次檢查；Vitest 新增延遲回應案例。
- 2026-10-26：code review 後補強日誌遮蔽：ADB 錯誤中以引號包住的序號（`device '…' not found`）與所有 `adb devices` 狀態列；`log.rs` 單元測試與斷線流程測試確認日誌不含序號。
