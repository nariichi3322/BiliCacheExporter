# 04: 單部影片安全匯出

Status: ready-for-agent

Resolution: resolved

**What to build:** 從清單選一部單畫質完整影片，匯出經真實影音工具驗證的 MP4；提供實際階段、取消及安全清理，既有影片不被覆寫。此票尚無封面時只能標示部分完成，不能假報全部完成。

**Blocked by:** 02 — 手動連線並掃描影片。

規格：[桌面介面與可攜發行改版](../spec.md)。

- [x] 前端匯出操作經 Rust 驗證所選項目、來源、裝置與授權；開始前裝置已拔除時不傳輸並要求重新連線，作業中斷線、程序失敗及逾時同樣正確處理。
- [x] 驗證影音工具、輸出／暫存可寫與相關磁碟空間，支援不同磁碟；只拉取選中來源，確認傳輸結果，來源變動或大小不符不可誤報成功。
- [x] 真實 FFprobe 辨識正片視訊與音訊，封面不能冒充正片；以 stream copy 合併 MP4，不轉碼、不猜測 DRM、不嘗試繞過 DRM。
- [x] 完整解碼驗證影音；容器可讀但內容損壞不能通過。有效影音無封面時保留 MP4 並標示部分完成，05 才補足完整成功路徑。
- [x] 安全處理 Unicode、Windows 非法字元、保留名稱及長檔名；同名加編號，最後落檔也不得覆寫既有檔案，失敗不留下冒充成品的檔案。
- [x] 介面顯示實際階段與可量測進度，無法量測時不造百分比；作業期間禁止重疊掃描或第二次匯出，取消保持可操作。
- [x] 使用者取消或程序錯誤時停止本次子程序，清理僅屬於本次作業的暫存，保留既有有效輸出，明確顯示取消／失敗及可查閱原因。
- [x] 用假手機傳輸搭配真實 FFmpeg／FFprobe，從匯出操作驗證正常 MP4、損壞封包、來源變動、斷線、空間／路徑錯誤、同名與取消；檢查來源未改、未選影音未傳輸。不需要真手機。

## Comments

- 2026-10-26：依使用者確認發布；不依賴 03，先以 02 的單畫質有效項目驗證完整路徑。
- 2026-10-26：完成並驗證（提交 dfb7e17；`temp_on_another_volume` 與 `cancel_during_cover_stage_leaves_no_output` 於後續提交加入，以 BILI_TEST_OTHER_VOLUME=D: 實跑通過）。ADB 為假手機，FFmpeg／FFprobe 為真實 9.0.1。
  - Rust 流程測試：`export_embeds_original_cover_and_verifies`（只拉選中項目的選定影音、真實 FFprobe 確認 H.264／AAC 正片、各階段通知、重複匯出得到「(1)」且原檔位元組不變、無殘留暫存）、`bad_media_never_reported_as_success`（封面冒充正片、全零檔、可探測但封包損壞——皆失敗且不留 MP4）、`changed_source_space_and_unwritable_output_fail_safely`（來源大小變動、空間不足於傳輸前拒絕、輸出路徑不可寫、自訂暫存資料夾）、`temp_on_another_volume`（暫存在 D:、輸出在 C:）、`reserved_and_long_names_are_safe`、`disconnect_before_and_during_export`（拔除後不傳輸並要求重新連線、重新連線不自動續跑、傳輸中斷線標示失敗並繼續下一部）、`cancel_stops_queue_keeps_finished_and_retry_works`、`cancel_during_real_ffmpeg_stops_process`（取消真實 FFmpeg 子程序；本機逾時不歸咎手機）、`overlapping_operations_are_refused`。
  - 最後落檔用 `MoveFileExW` 不帶取代旗標，確保即使競態也不覆寫既有檔案。
  - 正式 EXE 端到端：匯出期間顯示實際階段與整體進度（以項目數計），無法量測時為不定進度條；取消後保留先前輸出並清空本次暫存。
- 2026-10-26：08 打包驗證時發現並修正回歸（提交 5d3e151）：輸出資料夾較深且標題很長時，最終路徑超過 260 字元，直接呼叫的 `MoveFileExW` 未使用長路徑形式而失敗（Rust 標準檔案 API 本身會處理）。改為傳入 `\\?\` 長路徑形式，仍不帶取代旗標；`reserved_and_long_names_are_safe` 新增超過 260 字元目標的案例，並確認舊程式碼會失敗（「系統找不到指定的路徑」）、修正後通過。
