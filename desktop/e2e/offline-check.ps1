# Ticket 08 offline check, run by the user while the network is off (an assistant session cannot stay connected).
# Waits until the machine is really offline, runs the packaged e2e (fake phone + real FFmpeg, no download),
# confirms it stayed offline, and writes offline-check.json next to the package.
# Usage: .\desktop\e2e\offline-check.ps1 -Package 'dist-desktop\<stamp>'
param(
    [Parameter(Mandatory = $true)][string]$Package,
    [int]$WaitSeconds = 300
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path (Split-Path $PSScriptRoot)
$Package = (Resolve-Path -LiteralPath (Join-Path $repo $Package)).Path
$exe = Join-Path $Package 'BiliCacheExporter-win-x64\BiliCacheExporter.exe'
$fakeAdb = Join-Path $repo 'desktop\src-tauri\target\release\examples\fake_adb.exe'
$ffmpeg = (Import-PowerShellDataFile (Join-Path $repo '.check-local.psd1')).FFmpeg
foreach ($f in @($exe, $fakeAdb, $ffmpeg)) { if (!(Test-Path -LiteralPath $f)) { throw "找不到 $f" } }

function Test-Online {
    # Two independent HTTPS hosts; either answering means the network is still up.
    foreach ($url in @('https://dl.google.com/', 'https://www.microsoft.com/')) {
        try { Invoke-WebRequest -Uri $url -Method Head -TimeoutSec 5 -UseBasicParsing | Out-Null; return $true } catch { }
    }
    return $false
}

Write-Host '請現在中斷網路（拔網路線或關閉 Wi-Fi）。偵測到離線後會自動開始，不必再操作。' -ForegroundColor Yellow
$deadline = (Get-Date).AddSeconds($WaitSeconds)
while (Test-Online) {
    if ((Get-Date) -gt $deadline) { throw "等待 $WaitSeconds 秒仍可連線，未執行離線測試。" }
    Start-Sleep -Seconds 3
}
$started = Get-Date
$work = Join-Path $Package 'offline-check'
if (Test-Path -LiteralPath $work) { Remove-Item -LiteralPath $work -Recurse -Force }
New-Item -ItemType Directory -Path $work | Out-Null

# How long freshly copied / signed tools take to start while offline (a stall here explains a connect timeout).
Write-Host '已離線，先量測工具啟動時間…' -ForegroundColor Cyan
$freshCopy = Join-Path $work 'fresh-copy\adb.exe'
New-Item -ItemType Directory -Path (Split-Path $freshCopy) | Out-Null
Copy-Item -LiteralPath $fakeAdb -Destination $freshCopy
$googleAdb = Get-ChildItem -Path (Join-Path $repo 'dist-desktop\*\acceptance-09\BiliCacheExporter-win-x64\tools\adb\adb.exe') -ErrorAction SilentlyContinue | Select-Object -First 1
$launches = @(
    @{ name = 'fake_adb fresh copy (unsigned test double)'; exe = $freshCopy; args = 'version' },
    @{ name = 'Google adb.exe (signed)'; exe = $googleAdb.FullName; args = 'version' },
    @{ name = 'ffprobe'; exe = (Join-Path (Split-Path $ffmpeg) 'ffprobe.exe'); args = '-version' }
) | Where-Object { $_.exe } | ForEach-Object {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $p = Start-Process -FilePath $_.exe -ArgumentList $_.args -PassThru -WindowStyle Hidden
    $done = $p.WaitForExit(60000)
    if (!$done) { $p.Kill() }
    [ordered]@{ name = $_.name; seconds = [math]::Round($sw.Elapsed.TotalSeconds, 2); finished = $done }
}
$launches | ForEach-Object { Write-Host ("  {0}: {1}s{2}" -f $_.name, $_.seconds, $(if ($_.finished) { '' } else { '（60 秒未結束）' })) }

Write-Host '開始以正式包執行完整流程（約 2–3 分鐘，期間會開關程式視窗）…' -ForegroundColor Cyan
& node (Join-Path $PSScriptRoot 'package-e2e.mjs') $exe $fakeAdb $ffmpeg $work 2>&1 | Tee-Object -FilePath (Join-Path $work 'e2e-console.log')
$e2eExit = $LASTEXITCODE
$stillOffline = -not (Test-Online)
$results = Get-Content -LiteralPath (Join-Path $work 'evidence\e2e-results.json') -Raw -ErrorAction SilentlyContinue | ConvertFrom-Json
$report = [ordered]@{
    started = $started.ToString('o'); finished = (Get-Date).ToString('o')
    offlineAtStart = $true; offlineAtEnd = $stillOffline
    e2eExitCode = $e2eExit
    toolLaunches = $launches
    passed = @($results.results | Where-Object ok).Count; total = @($results.results).Count
    failed = @($results.results | Where-Object { -not $_.ok } | ForEach-Object name)
    package = (Split-Path $Package -Leaf)
}
$report | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $Package 'offline-check.json') -Encoding UTF8
$ok = $e2eExit -eq 0 -and $stillOffline
Write-Host ''
Write-Host ($(if ($ok) { "離線測試通過：$($report.passed)/$($report.total)。" } else { "離線測試未通過或中途恢復連線：$($report.passed)/$($report.total)，結束時離線=$stillOffline。" })) -ForegroundColor $(if ($ok) { 'Green' } else { 'Red' })
Write-Host '現在可以恢復網路，回到 Claude 說「離線測試跑完了」。' -ForegroundColor Yellow
