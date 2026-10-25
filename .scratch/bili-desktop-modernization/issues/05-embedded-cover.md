# 05: 內嵌封面與完成判定

Status: ready-for-agent

Resolution: resolved

**What to build:** 匯出時把原始封面嵌入 MP4，驗證影音與封面後才顯示完成；若只有封面失敗，保留有效影片並以部分完成說明結果。

**Blocked by:** 04 — 單部影片安全匯出。

規格：[桌面介面與可攜發行改版](../spec.md)。

- [x] 取得必要原始封面，以 attached picture 嵌入 MP4；不裁切、不加字，不在輸出目錄額外留下封面圖片。
- [x] 正片與封面在選取、合併及驗證階段保持區別；只有封面加音訊不能當成有效正片。
- [x] 原始封面內容保留，輸出能抽出對應原圖；只有影音完整解碼及封面驗證皆通過時，介面才顯示完成。
- [x] 缺封面、封面傳輸／格式／嵌入／驗證失敗但影音有效時保留 MP4，清楚標示部分完成及原因，不刪掉有效成果或回報全部成功。
- [x] 封面處理保持可取消與安全暫存清理；未完成輸出、同名保護與唯讀原則不因新階段失效。
- [x] 使用自製影音與原圖、假手機傳輸及真實影音工具，從匯出結果驗證成功、部分完成、封面冒充正片、取消及抽出封面內容。播放／檔案總管縮圖由 09 真機人工驗收，不需要本票接手機。

## Comments

- 2026-10-26：依使用者確認發布。
- 2026-10-26：完成並驗證（提交 64285be；`cancel_during_cover_stage_leaves_no_output` 於後續提交加入並通過）。
  - Rust 流程測試：`export_embeds_original_cover_and_verifies`（封面以 attached_pic 嵌入、寬高與 codec 相符，從 MP4 抽出的 JPEG 與手機原圖逐位元組相同，輸出資料夾不留封面圖片）、`cover_problems_keep_valid_video_as_partial`（缺封面與非圖片封面：保留通過完整解碼的 MP4，標示部分完成及原因）、`bad_media_never_reported_as_success`（只有封面＋音訊不能當正片）、`cancel_during_cover_stage_leaves_no_output`、`partial_retry_adds_numbered_file_without_overwriting`。
  - 正式 EXE 端到端：640×360 原始封面嵌入、抽出位元組相同；無封面影片顯示「部分完成 · 手機快取中沒有封面圖片」。
  - 播放器與檔案總管縮圖屬 09 人工驗收，本票未宣稱。
