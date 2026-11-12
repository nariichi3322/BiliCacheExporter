# Rebuilds the signed update packages used by the updater tests. Run from anywhere: .\regenerate.ps1
# The signatures come from the real `tauri signer sign`, so the tests prove the format the release workflow produces.
# Signing is not deterministic and Compress-Archive stamps the current time: commit the ZIPs and signatures together.
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
$desktop = Resolve-Path (Join-Path $here '..\..\..\..')
$key = Join-Path $desktop 'e2e\update-test-key\test-only.key'
$password = (Get-Content (Join-Path $desktop 'e2e\update-test-key\password.txt') -Raw).Trim()

$work = Join-Path ([IO.Path]::GetTempPath()) ("bili-fixture-" + [Guid]::NewGuid().ToString('N'))
$package = Join-Path $work 'BiliCacheExporter-win-x64'
New-Item -ItemType Directory -Path $package | Out-Null
[IO.File]::WriteAllText((Join-Path $package 'BiliCacheExporter.exe'), 'new exe')
[IO.File]::WriteAllText((Join-Path $package 'README.md'), 'new readme')

Remove-Item (Join-Path $here '*.zip'), (Join-Path $here '*.sig') -ErrorAction SilentlyContinue
# pwsh writes `/` separators (what the release workflow produces); Windows PowerShell 5 writes `\`.
Compress-Archive -LiteralPath $package -DestinationPath (Join-Path $here 'forward.zip')
powershell.exe -NoProfile -Command "Compress-Archive -LiteralPath '$package' -DestinationPath '$(Join-Path $here 'backslash.zip')'"
if ($LASTEXITCODE) { throw 'Windows PowerShell 5 could not write backslash.zip' }
Remove-Item $work -Recurse -Force

function Sign([string]$File, [string[]]$Extra) {
    Push-Location $desktop
    try {
        npx tauri signer sign -f $key -p $password @Extra $File | Out-Null
        if ($LASTEXITCODE -or !(Test-Path "$File.sig")) { throw "Signing $File failed" }
    }
    finally { Pop-Location }
}
# A signature without a version must be refused, so one is kept beside the versioned one.
Sign (Join-Path $here 'forward.zip') @()
Move-Item (Join-Path $here 'forward.zip.sig') (Join-Path $here 'forward.no-version.sig')
Sign (Join-Path $here 'forward.zip') @('--app-version', '99.0.0')
Sign (Join-Path $here 'backslash.zip') @('--app-version', '99.0.0')
Get-ChildItem $here -File | Select-Object Name, Length
