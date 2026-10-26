# Bili 快取匯出 2（Windows 國際版）

接上 Android 手機 → 按「連線」→ 勾選影片與畫質 → 匯出附原始封面的 MP4。

## 開始使用

1. 把整個資料夾解壓到可寫的位置，開啟 `BiliCacheExporter.exe`。
2. 需要 Microsoft Edge WebView2 Runtime（Windows 11 通常已內建）。若缺少，程式會先顯示原生提示並可開啟 Microsoft 官方下載頁。
3. 第一次按「連線」時若缺少 ADB 或 FFmpeg／FFprobe，會列出固定版本、來源與大小，可按「下載並設定」：檔案下載到程式旁的 `tools` 資料夾，核對 SHA-256 並確認能執行後才啟用。也可在「設定」指定自己已有的工具（不使用 PATH）。
4. 手機開啟 USB 偵錯、接上並解鎖，依手機提示允許這部電腦，再按「連線」。程式啟動與待機時不會查詢手機；多部裝置時先選擇目標。連線成功會自動掃描。
5. 清單初始不勾選。多畫質的項目先在同列選擇畫質；不完整、缺檔或不支援的項目會顯示原因且不能勾選。「全選」只選可匯出項目。
6. 按「匯出所選」。只傳輸所選畫質的影音，之後檢查、合併（不重新編碼）、完整解碼驗證並嵌入原始封面。

## 狀態

- **完成**：影音完整解碼與原始封面嵌入都通過檢查。
- **部分完成**：影音有效，但封面缺失或無法嵌入；有效 MP4 保留，可重試。
- **失敗**：該列與「詳細記錄」說明原因；單部失敗會繼續下一部。
- **取消**：停止目前與剩餘佇列，保留已完成影片並清理本次暫存。「重試未完成項目」會重新傳輸，不覆寫既有 MP4。手機拔除後需重新連線；重新連線不會自動恢復匯出。

## 資料夾內容與資料安全

- `settings.json`：設定。
- `log\`：每次執行一個記錄檔，不會自動上傳；裝置序號、網址參數與個人路徑會遮蔽，但影片標題仍可能出現。
- `tools\`：按需下載的 ADB（Google platform-tools r37.0.1）與 FFmpeg／FFprobe（gyan.dev essentials 9.0.1，GPLv3）。工具位置以相對路徑保存，搬移整個資料夾後仍可找到。
- `.bili-jobs\`：匯出暫存的歸屬紀錄；異常關閉後再次啟動會提示清理可確認屬於本程式的殘留。
- 全程唯讀手機快取，只處理 `com.bilibili.app.in` 的分離 M4S 快取；不登入、不 Root、不嘗試 DRM 解密或修補。
- `LICENSE` 是本專案的 MIT 授權；`licenses\THIRD-PARTY.txt` 收錄第三方依賴清單、授權全文、著作權聲明與原始碼來源（包含建置依賴，並非每個元件都進入 EXE）。WebView2 執行環境另存於 Windows 使用者資料夾（系統正常快取）。

## 從原始碼建置（開發者）

需要 Node.js 24、Rust（rustup stable，MSVC）與 Visual Studio C++ Build Tools，以及一份 FFmpeg（測試用，`ffprobe.exe` 在同目錄）。

```powershell
cd desktop
npm ci
npx tauri dev                                   # 開發模式
.\check.ps1 -FFmpeg 'D:\path\to\ffmpeg.exe'     # 型別、介面流程、Rust 單元與流程測試
.\package.ps1 -FFmpeg 'D:\path\to\ffmpeg.exe'   # 建置、驗證並產生 dist-desktop\<時間戳> 可攜包
```

`package.ps1` 會檢查包內容只有一個 EXE、量測啟動與閒置記憶體、模擬缺少 WebView2，並用測試專用的假手機（`src-tauri/examples/fake_adb.rs`，不隨包散布）搭配真實 FFmpeg 驅動正式 EXE。加 `-WithDownload` 會在正式包內實際下載 ADB。以上都不是真手機驗收。

提交前檢查：在 `.check-local.psd1`（不提交）以 PowerShell 資料檔格式設定 `FFmpeg` 執行檔完整路徑，執行 `git config core.hooksPath .githooks` 啟用；之後每次提交都會跑根目錄 `check.ps1`（即 `desktop/check.ps1`）。

## 開發協作

本專案在開發過程中使用了 OpenAI Codex 與 Anthropic Claude Code 協助撰寫程式與文件。

## 授權與使用責任

本專案自行撰寫的程式與文件採 [MIT 授權](LICENSE)。第三方元件維持各自授權，不因本專案使用 MIT 而改變。發行包附有第三方授權與聲明；原始碼建置者可執行 `node desktop/e2e/licenses.mjs <輸出檔>` 產生相同資料（需先安裝 npm 與 Cargo 依賴）。

第三方聲明也包含 WebView2 SDK 原生載入器的授權與 NOTICE；Rust 綁定的 MIT 不取代 Microsoft 元件的授權。Windows 上另行安裝的 WebView2 Runtime 依 Microsoft 的執行環境條款提供。

本專案與 Bilibili、Google 或 FFmpeg 無官方隸屬或背書關係。請僅處理你擁有權利或已獲授權的影音，並遵守適用法律與服務條款；本專案的 MIT 授權不授予影音內容或第三方商標的使用權。

軟體依「現狀」提供，不提供任何保證；在適用法律允許的範圍內，作者與著作權人不對使用本軟體造成的損害負責，完整條款以 LICENSE 為準。建議保留重要資料的備份。

ADB、相關 DLL 與 libwinpthread 的各別授權見下載後的 `tools/adb/NOTICE.txt`；Android SDK 的開源元件依其自身授權規範。FFmpeg／FFprobe 的授權見 `tools/ffmpeg/LICENSE`。這些工具由使用者按需自上游下載，不隨本專案發行包提供；重新散布含工具的資料夾時，必須另外遵守其授權（包括適用的原始碼提供義務）。
