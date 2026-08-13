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

$Version = (Select-String -Path (Join-Path $Root "Cargo.toml") -Pattern '^version = "([^"]+)"').Matches.Groups[1].Value
Write-Host "Embedding PE version info ($Version)..."
cargo run --quiet --release --manifest-path packaging/windows/embed-version/Cargo.toml -- $Out $Version

Write-Host "Built $Out"
Write-Host "Register: supersurfer.exe init --register"
Write-Host "Then Settings -> Apps -> Default apps -> SuperSurfer -> Set default"
