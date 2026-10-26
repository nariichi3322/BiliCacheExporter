# Portable package shape shared by package.ps1 (local, with GUI verification) and .github/workflows/ci.yml (release).
# Dot-source it, then call New-Package before any checks and Complete-Package after them.

# Fills $Package with the release EXE, README and licenses, checks its shape, and returns the EXE's --version text.
function New-Package([string]$Desktop, [string]$Package, [string]$Commit) {
    New-Item -ItemType Directory -Path (Join-Path $Package 'licenses') -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $Desktop 'src-tauri\target\release\BiliCacheExporter.exe'), (Join-Path (Split-Path $Desktop) 'README.md') -Destination $Package
    node (Join-Path $Desktop 'e2e\licenses.mjs') (Join-Path $Package 'licenses\THIRD-PARTY.txt')
    if ($LASTEXITCODE) { throw 'Writing licenses failed' }

    # One application EXE, no framework DLLs, no runtime state, tools or media.
    $files = @(Get-ChildItem -LiteralPath $Package -File -Recurse)
    $exes = @($files | Where-Object Extension -eq '.exe')
    $bad = @($files | Where-Object { $_.Extension -in @('.dll', '.m4s', '.mp4', '.jpg', '.png', '.pdb', '.log') -or $_.Name -eq 'settings.json' })
    if ($exes.Count -ne 1 -or $exes[0].Name -ne 'BiliCacheExporter.exe' -or $bad.Count -gt 0) { throw "Unexpected package contents: $($files.Name -join ', ')" }
    $version = (& (Join-Path $Package 'BiliCacheExporter.exe') --version | Out-String).Trim()
    if (!$version.Contains($Commit)) { throw "Version $version does not name commit $Commit" }
    $version
}

# Writes BUILD-INFO.json, zips $Package beside it with SHA256.txt, and returns the ZIP path.
function Complete-Package([string]$Package, [string]$Commit, [bool]$Dirty, [string]$Version) {
    $exe = Join-Path $Package 'BiliCacheExporter.exe'
    @{ Commit = $Commit; Dirty = $Dirty; Version = $Version; ExeSHA256 = (Get-FileHash $exe).Hash } | ConvertTo-Json | Set-Content (Join-Path $Package 'BUILD-INFO.json')
    $out = Split-Path $Package
    $zip = Join-Path $out 'BiliCacheExporter-win-x64.zip'
    Compress-Archive -LiteralPath $Package -DestinationPath $zip -CompressionLevel Optimal
    $hash = (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash
    "$hash  BiliCacheExporter-win-x64.zip" | Set-Content -LiteralPath (Join-Path $out 'SHA256.txt') -Encoding UTF8
    $zip
}
