# Required checks for the Tauri app: TypeScript types, UI flow tests, Rust unit/flow tests with real FFmpeg.
param([Parameter(Mandatory = $true)][string]$FFmpeg)
$ErrorActionPreference = 'Stop'
$env:Path = (Join-Path $env:USERPROFILE '.cargo\bin') + ';' + $env:Path
$env:BILI_TEST_FFMPEG = (Resolve-Path -LiteralPath $FFmpeg).Path
Push-Location $PSScriptRoot
try {
    if (!(Test-Path 'node_modules')) { npm ci; if ($LASTEXITCODE) { throw 'npm ci failed' } }
    npx vue-tsc --noEmit; if ($LASTEXITCODE) { throw 'Type check failed' }
    foreach ($script in Get-ChildItem 'e2e\*.mjs') { node --check $script.FullName; if ($LASTEXITCODE) { throw "Syntax error in $($script.Name)" } }
    npx vitest run; if ($LASTEXITCODE) { throw 'UI tests failed' }
    Push-Location 'src-tauri'
    try {
        cargo fmt --check; if ($LASTEXITCODE) { throw 'Rust formatting check failed (run cargo fmt)' }
        cargo clippy --quiet --all-targets -- -D warnings; if ($LASTEXITCODE) { throw 'Clippy failed' }
        cargo test --quiet; if ($LASTEXITCODE) { throw 'Rust tests failed' }
    }
    finally { Pop-Location }
    node --test e2e/licenses-check.mjs; if ($LASTEXITCODE) { throw 'License notice tests failed' }
    Write-Output 'Desktop checks passed.'
}
finally { Pop-Location }
