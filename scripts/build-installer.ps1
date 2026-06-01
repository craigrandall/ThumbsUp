# Build the entire ThumbsUp release: x64 DLL, x64 GUI, MSI installer.
# Run from the repository root in a Developer PowerShell where cargo, candle,
# and light are on PATH.

$ErrorActionPreference = "Stop"

# PowerShell's $ErrorActionPreference does NOT make native-exe non-zero exits
# terminating, so we have to check $LASTEXITCODE explicitly after each native
# invocation. A previous version of this script printed "==> Done" even when
# light.exe failed ICE validation; this helper makes that class of bug
# impossible.
function Invoke-Native {
    param([string]$Tool, [string[]]$ArgList)
    & $Tool @ArgList
    if ($LASTEXITCODE -ne 0) {
        throw "$Tool exited with code $LASTEXITCODE"
    }
}

Write-Host "==> Building Rust workspace (release, x64)..."
Invoke-Native cargo @(
    "build", "--release",
    "--target", "x86_64-pc-windows-msvc",
    "-p", "thumbsup-shell",
    "-p", "thumbsup-config"
)

$dll = "target\x86_64-pc-windows-msvc\release\thumbsup_shell.dll"
$exe = "target\x86_64-pc-windows-msvc\release\thumbsup-config.exe"

if (-not (Test-Path $dll)) { throw "DLL not found: $dll" }
if (-not (Test-Path $exe)) { throw "EXE not found: $exe" }

Write-Host "==> Compiling WiX authoring..."
Invoke-Native candle.exe @(
    "-arch", "x64",
    "-dDllPath=$((Resolve-Path $dll).Path)",
    "-dConfigExePath=$((Resolve-Path $exe).Path)",
    "-out", "installer\Product.wixobj",
    "installer\Product.wxs"
)

Write-Host "==> Linking MSI..."
Invoke-Native light.exe @(
    "-ext", "WixUIExtension",
    "-cultures:en-us",
    "-loc", "installer\en-us.wxl",
    "-b", "installer",
    "-out", "ThumbsUp.msi",
    "installer\Product.wixobj"
)

Write-Host "==> Done. Output: ThumbsUp.msi"
Write-Host "    Sign with: signtool.exe sign /fd SHA256 /a ThumbsUp.msi"
