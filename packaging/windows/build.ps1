#Requires -Version 5.1
$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
Set-Location $Root
$env:CARGO_TARGET_DIR = Join-Path $Root "target"

Write-Host "Building release binary..."
cargo build --release

$Dist = Join-Path $Root "dist"
New-Item -ItemType Directory -Force -Path $Dist | Out-Null

$Exe = Join-Path $Root "target\release\supersurfer.exe"
$Out = Join-Path $Dist "supersurfer.exe"
Copy-Item -Force $Exe $Out

Write-Host "Built $Out"
Write-Host "Register: supersurfer.exe init --register"
Write-Host "Then Settings -> Apps -> Default apps -> SuperSurfer -> Set default"
