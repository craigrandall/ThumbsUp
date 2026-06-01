# Per-user register/unregister helper. Use during development to test the
# DLL without building the full MSI.
#
# Usage:
#   .\register.ps1                       # register per-user (no admin)
#   .\register.ps1 -Unregister           # unregister
#   .\register.ps1 -Machine              # per-machine (requires elevation)

param(
    [switch]$Unregister,
    [switch]$Machine,
    [string]$Dll = "target\x86_64-pc-windows-msvc\release\thumbsup_shell.dll"
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path $Dll)) {
    throw "DLL not found at $Dll. Build it first: cargo build --release --target x86_64-pc-windows-msvc -p thumbsup-shell"
}

$dllAbs = (Resolve-Path $Dll).Path
Write-Host "DLL: $dllAbs"

$flags = @("/s")
if ($Unregister) { $flags += "/u" }
if (-not $Machine) {
    # /n + /i:user => skip machine-wide DllRegisterServer, do per-user via custom path
    $flags += "/n", "/i:user"
}

& regsvr32.exe @flags $dllAbs
$rc = $LASTEXITCODE
if ($rc -eq 0) {
    if ($Unregister) { Write-Host "Unregistered." } else { Write-Host "Registered." }
} else {
    Write-Host "regsvr32 returned $rc"
}

# Restart Explorer so the change is picked up. Cosmetic; users can also
# log off and back on.
Stop-Process -Name explorer -Force -ErrorAction SilentlyContinue
Start-Process explorer
