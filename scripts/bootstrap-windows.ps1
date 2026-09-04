[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
$toolchain = "1.95.0-x86_64-pc-windows-msvc"
$env:RUSTUP_NO_UPDATE_CHECK = "1"

if ([System.Environment]::OSVersion.Platform -ne [System.PlatformID]::Win32NT) {
    throw "scripts/bootstrap-windows.ps1 is only supported on Windows."
}

if (-not (Get-Command rustup -ErrorAction SilentlyContinue)) {
    throw "rustup is required. Install rustup before bootstrapping RuViE."
}

Write-Host "[bootstrap] installing Rust $toolchain"
rustup toolchain install $toolchain --profile minimal --component clippy --component rustfmt --no-self-update
if ($LASTEXITCODE -ne 0) {
    throw "rustup could not install $toolchain."
}

Write-Host "[bootstrap] selecting the MSVC toolchain for this repository"
rustup override set $toolchain
if ($LASTEXITCODE -ne 0) {
    throw "rustup could not set the repository override to $toolchain."
}

$hostLine = rustc -vV | Select-String '^host:'
if ($hostLine -notmatch 'x86_64-pc-windows-msvc') {
    throw "RuViE on Windows requires the MSVC Rust host; active compiler reports '$hostLine'."
}

Write-Host "[bootstrap] Rust MSVC toolchain is ready. Run: cargo build --release"
