# Runs cargo with the portable MSVC Build Tools environment, if configured.
# Usage: ./dev.ps1 build --release   |   ./dev.ps1 test
# Set RPAD_MSVC_ENV to the path of a devcmd.ps1 (portable Build Tools) when
# Visual Studio is not installed system-wide.
$envScript = if ($env:RPAD_MSVC_ENV) { $env:RPAD_MSVC_ENV } else { 'D:\Portable\Tools\dev\msvc\devcmd.ps1' }
if (-not $env:VCToolsInstallDir -and (Test-Path $envScript)) {
    . $envScript | Out-Null
}
cargo @args
exit $LASTEXITCODE
