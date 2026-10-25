# 02: 手動連線並掃描影片

Status: ready-for-agent

Resolution: resolved

**What to build:** 使用手動指定的 ADB，按連線才發現與選定手機，成功後自動唯讀掃描國際版快取，在精簡清單顯示真實影片資訊與初始未勾選項目。單畫質完整項目能成為後續匯出來源。

**Blocked by:** 01 — 新版啟動、設定與深淺色。

規格：[桌面介面與可攜發行改版](../spec.md)。

- [x] 啟動與待機期間不查詢裝置，無背景輪巡或常駐監視。按連線才列舉；單一已授權裝置可連線並掃描，多裝置必須明確選擇。
- [x] 沒有裝置、未授權、offline、ADB 啟動失敗與逾時有不同且可操作的提示，使用者修正後可再次連線；不自動恢復匯出。
- [x] 操作綁定所選序號，套件查詢限定 Android user 0，只掃描國際版，驗證遠端路徑與安全引數；手機上只讀取，不寫入、修補或刪除。
- [x] 連線成功自動掃描並呈現封面、真實標題／分 P、已取得的大小及畫質資訊；每個分 P／集數一列，不固定品質目錄名稱。不虛構作者、時長等缺少資訊。
- [x] 初始不勾選；單一完整畫質可指定為來源；損壞中繼資料限於該項目並顯示原因，不能讓其他有效影片消失。多畫質在 03 完成前不可默選來源。
- [x] 手動重新掃描前再次檢查裝置；切換裝置使舊清單失效，不把舊手機選取帶入新手機。操作中避免重疊連線／掃描，離線與失敗不留下可誤操作的有效狀態。
- [x] 接入實際掃描階段／結果通知與詳細記錄，兩種主題下提示及停用狀態可讀；不複製原型假百分比。
- [x] 使用假手機從連線操作驗證裝置錯誤、單／多裝置、空快取、Unicode、掃描中斷線、來源唯讀、無影音拉取及閒置不查詢；不需要真手機。

## Comments

- 2026-10-26：依使用者確認發布；進階畫質及完整無效快取盤點由 03 接續。
- 2026-10-26：完成並驗證（提交 64285be）。全程使用假手機，未接真手機。
  - Rust 流程測試：`device_problems_have_distinct_messages`（無裝置／未授權／offline／ADB 逾時／ADB 無法啟動，修正後可再連線）、`multiple_devices_need_an_explicit_choice`（多裝置不讀任何手機，選定後所有命令綁定該序號）、`scan_is_read_only_shows_real_metadata_and_isolates_bad_rows`（Unicode 標題＋分 P、真實封面、動態品質目錄、壞 entry.json 只影響該列、越界路徑不保留為來源、掃描只拉 cover.jpg、手機只收到 pm list／find／stat／cat、日誌不含序號）、`empty_cache_and_missing_app`、`rescan_rechecks_device_and_switching_invalidates_list`、`disconnect_during_scan_leaves_nothing_exportable`、`overlapping_operations_are_refused`、`startup_and_idle_never_query_devices`。
  - 正式 EXE 端到端：鍵盤 Tab 到「連線」按 Enter 後自動掃描列出 4 列、該動作只列舉裝置 1 次、全部初始未勾選、有封面者顯示真實封面、未完成與待選畫質顯示原因；不顯示虛構作者／時長；掃描期間只顯示階段文字與已找到數量，不造百分比。
