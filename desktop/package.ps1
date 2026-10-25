# Builds and verifies the Tauri portable package into a new dist-desktop/<stamp> folder (never reuses one).
# Usage: .\desktop\package.ps1 -FFmpeg 'D:\...\ffmpeg.exe' [-WithDownload]
param(
    [Parameter(Mandatory = $true)][string]$FFmpeg,
    [switch]$WithDownload
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$desktop = $PSScriptRoot
$repo = Split-Path $desktop
$FFmpeg = (Resolve-Path -LiteralPath $FFmpeg).Path
if (!(Test-Path -LiteralPath (Join-Path (Split-Path $FFmpeg) 'ffprobe.exe'))) { throw 'ffprobe.exe must be beside ffmpeg.exe.' }
$env:Path = (Join-Path $env:USERPROFILE '.cargo\bin') + ';' + $env:Path
function Invoke-Checked([string]$File, [string[]]$Arguments) {
    & $File @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$File failed ($LASTEXITCODE): $Arguments" }
}

Push-Location $desktop
try {
    & (Join-Path $desktop 'check.ps1') -FFmpeg $FFmpeg
    Invoke-Checked 'npx' @('tauri', 'build')
    Push-Location 'src-tauri'
    try { Invoke-Checked 'cargo' @('build', '--release', '--example', 'fake_adb') } finally { Pop-Location }
    $built = Join-Path $desktop 'src-tauri\target\release\BiliCacheExporter.exe'
    $fakeAdb = Join-Path $desktop 'src-tauri\target\release\examples\fake_adb.exe'

    $commit = (& git -C $repo rev-parse HEAD).Trim()
    $dirty = [bool](& git -C $repo status --porcelain)
    $stamp = ([DateTimeOffset]::UtcNow.ToOffset([TimeSpan]::FromHours(8))).ToString('yyyyMMdd-HHmmss') + '-' + [Guid]::NewGuid().ToString('N').Substring(0, 8)
    $run = Join-Path $repo "dist-desktop\$stamp"
    if (Test-Path -LiteralPath $run) { throw 'Refusing to reuse an artifact directory.' }
    $package = Join-Path $run 'BiliCacheExporter-win-x64'
    $evidence = Join-Path $run 'evidence'
    New-Item -ItemType Directory -Path $package, $evidence, (Join-Path $package 'licenses') -Force | Out-Null
    Copy-Item -LiteralPath $built -Destination $package
    Copy-Item -LiteralPath (Join-Path $desktop 'README.md') -Destination $package
    Invoke-Checked 'node' @((Join-Path $desktop 'e2e\licenses.mjs'), (Join-Path $package 'licenses\THIRD-PARTY.txt'))

    # Package shape: one application EXE, no framework DLLs, no runtime state, tools or media.
    $files = @(Get-ChildItem -LiteralPath $package -File -Recurse)
    $exes = @($files | Where-Object Extension -eq '.exe')
    $bad = @($files | Where-Object { $_.Extension -in @('.dll', '.m4s', '.mp4', '.jpg', '.png', '.pdb', '.log') -or $_.Name -eq 'settings.json' })
    if ($exes.Count -ne 1 -or $exes[0].Name -ne 'BiliCacheExporter.exe' -or $bad.Count -gt 0) { throw "Unexpected package contents: $($files.Name -join ', ')" }
    $version = (& (Join-Path $package 'BiliCacheExporter.exe') --version | Out-String).Trim()
    if (!$version.Contains($commit)) { throw "Version $version does not name commit $commit" }

    # Startup to operable window (UI mounted and shown), three fresh runs; excludes connect/scan.
    $startup = @()
    foreach ($i in 1..3) {
        $probeDir = Join-Path $run "startup-$i"
        New-Item -ItemType Directory -Path $probeDir | Out-Null
        Copy-Item -LiteralPath (Join-Path $package 'BiliCacheExporter.exe') -Destination $probeDir
        $out = Join-Path $probeDir 'startup-ms.txt'
        $p = Start-Process (Join-Path $probeDir 'BiliCacheExporter.exe') -ArgumentList '--measure-startup', "`"$out`"" -PassThru -WorkingDirectory $probeDir
        if (!$p.WaitForExit(60000)) { $p.Kill(); throw 'Startup measurement timed out.' }
        $startup += [double](Get-Content -LiteralPath $out)
        Remove-Item -LiteralPath $probeDir -Recurse -Force
    }
    $median = ($startup | Sort-Object)[1]

    # Idle memory: the app process and its WebView2 descendants after 8 s without any action.
    $idleDir = Join-Path $run 'idle'
    New-Item -ItemType Directory -Path $idleDir | Out-Null
    Copy-Item -LiteralPath (Join-Path $package 'BiliCacheExporter.exe') -Destination $idleDir
    $p = Start-Process (Join-Path $idleDir 'BiliCacheExporter.exe') -PassThru -WorkingDirectory $idleDir
    Start-Sleep -Seconds 8
    $all = @(Get-CimInstance Win32_Process)
    $tree = @($p.Id); $added = $true
    while ($added) { $added = $false; foreach ($proc in $all) { if ($tree -contains $proc.ParentProcessId -and $tree -notcontains $proc.ProcessId) { $tree += $proc.ProcessId; $added = $true } } }
    $procs = @(Get-Process -Id $tree -ErrorAction SilentlyContinue)
    $appWs = (Get-Process -Id $p.Id).WorkingSet64
    $treeWs = ($procs | Measure-Object WorkingSet64 -Sum).Sum
    $treePrivate = ($procs | Measure-Object PrivateMemorySize64 -Sum).Sum
    $p.Kill(); $p.WaitForExit()
    Start-Sleep -Seconds 1
    Remove-Item -LiteralPath $idleDir -Recurse -Force

    # Missing WebView2 (simulated by pointing the loader at an empty runtime folder).
    $wvDir = Join-Path $run 'no-webview2'
    New-Item -ItemType Directory -Path (Join-Path $wvDir 'empty-runtime') -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $package 'BiliCacheExporter.exe') -Destination $wvDir
    $env:WEBVIEW2_BROWSER_EXECUTABLE_FOLDER = Join-Path $wvDir 'empty-runtime'
    try { $p = Start-Process (Join-Path $wvDir 'BiliCacheExporter.exe') -PassThru -WorkingDirectory $wvDir } finally { Remove-Item Env:\WEBVIEW2_BROWSER_EXECUTABLE_FOLDER }
    Start-Sleep -Seconds 3; $p.Refresh()
    $wvTitle = $p.MainWindowTitle
    $wvChildren = @(Get-CimInstance Win32_Process -Filter "ParentProcessId=$($p.Id)").Count
    # Answer 「否」: the app must exit (code 2) without writing anything.
    $shell = New-Object -ComObject WScript.Shell
    if ($shell.AppActivate($p.Id)) { Start-Sleep -Milliseconds 300; $shell.SendKeys('n') }
    if (!$p.WaitForExit(10000)) { $p.Kill(); $p.WaitForExit(); throw 'WebView2 guidance did not close on 否.' }
    $wvExit = $p.ExitCode
    $wvWrote = @(Get-ChildItem -LiteralPath $wvDir -Name | Where-Object { $_ -notin @('BiliCacheExporter.exe', 'empty-runtime') })
    if ($wvTitle -ne '需要安裝 WebView2' -or $wvChildren -ne 0 -or $wvWrote.Count -ne 0 -or $wvExit -ne 2) { throw "WebView2 guidance check failed: '$wvTitle' children=$wvChildren wrote=$wvWrote exit=$wvExit" }
    Remove-Item -LiteralPath $wvDir -Recurse -Force

    # Integrated flows on the packaged EXE: fake phone + real FFmpeg/FFprobe.
    $e2e = Join-Path $run 'e2e'
    $e2eArgs = @((Join-Path $desktop 'e2e\package-e2e.mjs'), (Join-Path $package 'BiliCacheExporter.exe'), $fakeAdb, $FFmpeg, $e2e)
    if ($WithDownload) { $e2eArgs += '--with-download' }
    Invoke-Checked 'node' $e2eArgs
    Copy-Item -Path (Join-Path $e2e 'evidence\*') -Destination $evidence
    Remove-Item -LiteralPath $e2e -Recurse -Force
    $e2eResults = Get-Content -LiteralPath (Join-Path $evidence 'e2e-results.json') -Raw | ConvertFrom-Json

    $exe = Get-Item -LiteralPath (Join-Path $package 'BiliCacheExporter.exe')
    @{ Commit = $commit; Dirty = $dirty; Version = $version; ExeSHA256 = (Get-FileHash $exe).Hash } | ConvertTo-Json | Set-Content (Join-Path $package 'BUILD-INFO.json')
    $zip = Join-Path $run 'BiliCacheExporter-win-x64.zip'
    Compress-Archive -LiteralPath $package -DestinationPath $zip -CompressionLevel Optimal
    $hash = (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash
    "$hash  BiliCacheExporter-win-x64.zip" | Set-Content -LiteralPath (Join-Path $run 'SHA256.txt') -Encoding UTF8
    $packageBytes = (Get-ChildItem -LiteralPath $package -File -Recurse | Measure-Object Length -Sum).Sum
    $mib = { param($b) [Math]::Round($b / 1MB, 1) }
    $report = @(
        '# 新版可攜包驗證紀錄（本機包／假手機／環境測試）', '',
        ('時間：' + [DateTimeOffset]::UtcNow.ToOffset([TimeSpan]::FromHours(8)).ToString('yyyy-MM-dd HH:mm:ss zzz')),
        "版本：$version", '',
        "- 包內容：$((Get-ChildItem -LiteralPath $package -File -Recurse -Name) -join '、')。唯一 EXE，無框架 DLL；不含 ADB／FFmpeg。",
        "- EXE $($exe.Length) bytes（$(& $mib $exe.Length) MiB）；解壓 $packageBytes bytes；ZIP $((Get-Item $zip).Length) bytes；SHA-256 $hash。",
        '- 工具下載（固定版本，按需）：ADB r37.0.1 8,044,989 bytes；FFmpeg 9.0.1 essentials 111,253,802 bytes。落地 tools 資料夾約 215,396,052 bytes（2026-10-26 實測）。',
        "- 啟動到視窗可操作（UI 掛載並顯示，不含連線／掃描），三次新資料夾：$(($startup | ForEach-Object { [Math]::Round($_) }) -join '、') ms；中位數 $([Math]::Round($median)) ms。",
        "- 閒置 8 秒、未連線：應用程序 Working Set $(& $mib $appWs) MiB；含 WebView2 子程序共 $($procs.Count) 個程序，Working Set 合計 $(& $mib $treeWs) MiB、Private 合計 $(& $mib $treePrivate) MiB。WebView2 子程序共享系統元件，合計值會重複計算共享頁面。",
        '- 與舊版比較：舊 WinForms 量測為「正常視窗閒置 Working Set 約 62 MiB、開窗中位數約 0.34 秒」，只量單一 .NET 程序；新版另含 WebView2 多程序，量測範圍不同，不宣稱更快或更省記憶體。',
        "- WebView2 缺少（以空的 WEBVIEW2_BROWSER_EXECUTABLE_FOLDER 模擬）：顯示原生「需要安裝 WebView2」對話框，未啟動 WebView 子程序；按「否」以代碼 2 結束且未寫入任何檔案（「是」只開啟 Microsoft 官方頁，未自動化點擊）。不是乾淨虛擬機上的實測。",
        "- 正式包整合流程：$(@($e2eResults.results | Where-Object ok).Count)/$(@($e2eResults.results).Count) PASS（假手機＋真實 FFmpeg/FFprobe，DPR $($e2eResults.devicePixelRatio)）。截圖在 evidence/。",
        $(if ($WithDownload) { '- 本次包含正式包內的真實 ADB 下載、SHA-256 核對與啟用。' } else { '- 本次未在正式包內執行真實下載（加 -WithDownload 才會）。' }),
        '- 未驗證：真手機、實際播放、封面外觀與檔案總管縮圖（工作票 09，由使用者確認）。'
    )
    $report | Set-Content -LiteralPath (Join-Path $run 'VERIFICATION.md') -Encoding UTF8
    Write-Output "Package verified: $run"
}
finally { Pop-Location }
