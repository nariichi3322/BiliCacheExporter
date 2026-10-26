param([switch]$SkipRestore)
$ErrorActionPreference = 'Stop'
$config = Join-Path $PSScriptRoot '.check-local.psd1'
if (!(Test-Path $config)) { throw 'Create .check-local.psd1 with the FFmpeg path; see README.' }
$tools = Import-PowerShellDataFile $config
& (Join-Path $PSScriptRoot 'desktop\check.ps1') -FFmpeg $tools.FFmpeg
if (!$?) { throw 'Desktop checks failed.' }
