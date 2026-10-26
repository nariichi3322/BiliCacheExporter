# Bili Cache Exporter (Android app → Windows)

[繁體中文](README.md) | English

Connect your Android phone → press Connect → pick videos and quality → export MP4 files with their original covers.

Supports the cache of both the international (`com.bilibili.app.in`) and China (`tv.danmaku.bili`) Bilibili apps.

The app's interface is in Traditional Chinese; the button names below give the Chinese label in brackets.

## Getting started

1. Extract the whole folder to a writable location and open `BiliCacheExporter.exe`.
2. Microsoft Edge WebView2 Runtime is required (usually built into Windows 11). If it is missing, the program shows a native prompt that can open Microsoft's official download page.
3. If ADB or FFmpeg/FFprobe is missing the first time you press Connect (「連線」), the program lists the pinned version, source and size of each tool. Press Download and set up (「下載並設定」): files are downloaded to the `tools` folder next to the program and enabled only after their SHA-256 is verified and they are confirmed to run. You can also point Settings (「設定」) at tools you already have (PATH is not used).
4. Enable USB debugging on the phone, connect and unlock it, allow this computer when the phone asks, then press Connect. The program never queries the phone at startup or while idle; with several devices, choose the target first. A successful connection scans automatically.
5. Nothing is selected at first. For items with several qualities, choose the quality in the same row first; incomplete, damaged or unsupported items show the reason and cannot be selected. Select all only selects exportable items.
6. Press Export selected (「匯出所選」). Only the chosen quality's audio and video are transferred, then checked, merged (no re-encoding), fully decoded for verification and given the original cover.

## Results

- **Done**: the audio/video fully decoded and the original cover was embedded.
- **Partly done**: the audio/video is valid but the cover is missing or could not be embedded; the valid MP4 is kept and you can retry.
- **Failed**: the row and the detailed log (「詳細記錄」) give the reason; one failure does not stop the rest.
- **Cancelled**: stops the current and remaining items, keeps finished videos and cleans this run's temporary files. Retry unfinished items (「重試未完成項目」) transfers again and never overwrites existing MP4 files. After unplugging the phone you must reconnect; reconnecting does not resume an export by itself.

## Folder contents and data safety

- `settings.json`: settings.
- `log\`: one log file per run, never uploaded automatically; device serials, URL parameters and personal paths are masked, but video titles may still appear.
- Network: by default each start asks GitHub whether a newer version exists (no logs or phone data are sent); turn off Check for updates at startup (「啟動時檢查更新」) under Settings (「設定」) → Update (「更新」). Tools are downloaded only when you press Download.
- `tools\`: ADB (Google platform-tools r37.0.1) and FFmpeg/FFprobe (gyan.dev essentials 9.0.1, GPLv3), downloaded only when you ask. Tool locations are stored as relative paths, so they are still found after moving the whole folder.
- `.bili-jobs\`: ownership records for temporary export files; after an abnormal exit, the next start offers to clean leftovers that provably belong to this program.
- The phone's cache is only ever read. Only the separate M4S cache of the international app `com.bilibili.app.in` and the China app `tv.danmaku.bili` is handled (a video cached by both apps is one row whose qualities are chosen together; Cache details (「快取詳情」) shows the source); no login, no root, and no attempt to decrypt or patch DRM.
- `LICENSE` is this project's MIT license; `licenses\THIRD-PARTY.txt` lists third-party dependencies with full license texts, copyright notices and source locations (build dependencies included, so not every component ends up in the EXE). The WebView2 runtime keeps its own data in the Windows user folder (normal system cache).

## Building from source (developers)

Requires Node.js 24, Rust (rustup stable, MSVC), Visual Studio C++ Build Tools, and a copy of FFmpeg for the tests (with `ffprobe.exe` in the same folder).

```powershell
cd desktop
npm ci
npx tauri dev                                   # development mode
.\check.ps1 -FFmpeg 'D:\path\to\ffmpeg.exe'     # types, UI flows, Rust unit and flow tests
.\package.ps1 -FFmpeg 'D:\path\to\ffmpeg.exe'   # build, verify and produce a portable package in dist-desktop\<timestamp>
```

`package.ps1` checks that the package holds a single EXE, measures startup and idle memory, simulates missing WebView2, and drives the release EXE with a test-only fake phone (`src-tauri/examples/fake_adb.rs`, not shipped) and real FFmpeg. Adding `-WithDownload` also downloads ADB for real inside the package. None of this replaces testing with a real phone.

To rerun only the UI flows (not covered by `check.ps1`; needed after changing on-screen text, buttons or flows): in `desktop`, run `npx tauri build` and `cargo build --release --example fake_adb` (in `src-tauri`), then `node e2e\package-e2e.mjs src-tauri\target\release\BiliCacheExporter.exe src-tauri\target\release\examples\fake_adb.exe 'D:\path\to\ffmpeg.exe' <new empty folder>`. This skips the in-app update run; `package.ps1` remains the full verification.

Pre-commit checks: set the full path of the `FFmpeg` executable in `.check-local.psd1` (a PowerShell data file, not committed), then run `git config core.hooksPath .githooks`; every commit then runs the root `check.ps1` (which calls `desktop/check.ps1`).

## Development assistance

OpenAI Codex and Anthropic Claude Code were used to help write the code and documentation of this project.

## License and responsibility

The code and documentation written for this project are under the [MIT License](LICENSE). Third-party components keep their own licenses, which this project's MIT license does not change. Release packages include the third-party licenses and notices; when building from source you can generate the same data with `node desktop/e2e/licenses.mjs <output file>` (after installing the npm and Cargo dependencies).

The third-party notices also cover the license and NOTICE of the WebView2 SDK's native loader; the MIT license of the Rust bindings does not replace the license of Microsoft's components. The WebView2 Runtime installed separately on Windows is provided under Microsoft's runtime terms.

This project is not affiliated with or endorsed by Bilibili, Google or FFmpeg. Only process audio and video you own or are authorized to use, and follow applicable laws and terms of service; this project's MIT license grants no rights to video content or third-party trademarks.

The software is provided "as is", without warranty of any kind; to the extent permitted by applicable law, the authors and copyright holders are not liable for any damage arising from its use. The full terms are in LICENSE. Keep backups of important data.

The individual licenses of ADB, its DLLs and libwinpthread are in `tools/adb/NOTICE.txt` after download; the Android SDK's open-source components follow their own licenses. The FFmpeg/FFprobe license is in `tools/ffmpeg/LICENSE`. These tools are downloaded by the user from upstream on demand and are not part of this project's release package; if you redistribute a folder that contains them, you must comply with their licenses yourself (including any obligation to provide source code).
