# 07: 缺少工具時引導下載

Status: ready-for-agent

Resolution: resolved

**What to build:** 缺少必要工具時，使用者可看清楚下載項目並按下載設定，成功驗證後直接使用；網路失敗時可取消、重試或手動指定已有工具。

**Blocked by:** 01 — 新版啟動、設定與深淺色。

規格：[桌面介面與可攜發行改版](../spec.md)。

- [x] 選定並記錄可信 Windows 工具來源、固定版本、架構、實際大小、必要支援檔與再散布／授權資訊；涵蓋 ADB 及 FFmpeg／FFprobe，不使用原型假大小或浮動最新版。
- [x] 在相關功能需要且缺少工具時引導，列出缺少項目、版本、來源、大小與下載按鈕；正常啟動不自動查更新，使用者未觸發時不自行下載。
- [x] 下載經 HTTPS 到專用暫存，使用隨應用版本發行的可信 SHA-256 檢查；解壓拒絕越界路徑，驗證必要檔案完整及工具可執行後才啟用。
- [x] 工具集中存於程式旁工具資料夾，內建工具位置採可攜方式保存，搬移後仍可找到；ADB 必要 DLL 不散落程式根目錄。完成設定後無網路仍能使用工具。
- [x] 顯示可取得的下載／驗證進度並可取消；中斷、錯誤雜湊、磁碟滿、不可寫、缺檔與啟動失敗有明確提示及重試／手動指定入口，不毀損已可用工具。
- [x] 手動設定與下載共用工具可用性驗證及結果，不另建一套核心；只補必要下載替換點供可重現測試，不依賴公開網站在線才能跑測試。
- [x] 從工具設定操作測試成功、取消、失敗重試、錯誤雜湊、解壓越界、不可寫、手動替代及舊工具保留；實際下載來源另做取得與版本驗證。全程不需要真手機。

## Comments

- 2026-10-26：依使用者確認發布；可在 01 完成後先推進，不必等待掃描或匯出。
- 2026-10-26：完成並驗證（提交 64285be）。
  - 選定來源（2026-10-26 下載並核對發行者校驗值）：
    - ADB：Android SDK Platform-Tools r37.0.1（Google，`https://dl.google.com/android/repository/platform-tools_r37.0.1-win.zip`），8,044,989 bytes，SHA-256 `45f4d631…1a99`（Google repository2-3.xml 的 SHA-1 `e03e78b1…1110` 相符）。啟用 adb.exe、AdbWinApi.dll、AdbWinUsbApi.dll、libwinpthread-1.dll、NOTICE.txt、source.properties。Android SDK License；由使用者自 Google 下載，不隨包再散布。
    - FFmpeg／FFprobe：gyan.dev essentials 9.0.1（GitHub GyanD/codexffmpeg 標籤 9.0.1），111,253,802 bytes，SHA-256 `fec81ae0…a2e9`（與 GitHub 發行資產 digest 相符）。啟用 ffmpeg.exe、ffprobe.exe、LICENSE（GPLv3）。x64，固定版本，不追最新。
    - 落地 tools 資料夾合計 215,396,052 bytes。
  - 只在操作需要且缺少工具時由錯誤的 `missing` 欄位開啟引導；啟動不查更新、不自動下載。HTTPS-only（`https_only`）、專用 `.download-*` 暫存、SHA-256 與大小核對、zip 項目越界即拒絕、必要檔案缺一即拒絕、實際執行驗證後才以改名替換 `tools/<name>`；改名遇 Windows 防毒短暫鎖檔時有限次重試（實際下載測試發現並修正）。
  - Rust 測試（`tests/tools_download.rs`，下載來源以本機替換）：成功替換、錯誤雜湊、中斷、取消、缺檔、越界、不可執行、大小不符——每種失敗後舊工具內容不變、暫存清空、修正後重試成功；tools 不可寫；應用層下載失敗不改設定且可重試或手動指定；工具以相對路徑保存，搬移整個資料夾後仍找到並可離線連線；非 HTTPS 被拒。磁碟滿未以真實滿磁碟模擬，與中斷／不可寫走同一寫入錯誤路徑並顯示「磁碟空間或權限」。
  - 真實來源：`real_pinned_downloads_install_and_run`（--ignored）實際下載兩個固定套件並執行，得到「Android Debug Bridge version 1.0.41」「ffmpeg version 9.0.1-essentials_build-www.gyan.dev」「ffprobe version 9.0.1-essentials_build-www.gyan.dev」。正式 EXE 端到端也在包內按「下載並設定」實際取得並啟用 ADB。手動指定與下載共用 `check_tool`。
