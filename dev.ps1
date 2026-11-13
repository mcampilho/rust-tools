# Runs cargo with the portable MSVC Build Tools environment, if configured.
# Usage: ./dev.ps1 dist                  release builds of all the apps, in dist\
#        ./dev.ps1 build --release -p rview
#        ./dev.ps1 test --workspace
# Set RUST_TOOLS_MSVC_ENV to the path of a devcmd.ps1 (portable Build Tools)
# when Visual Studio is not installed system-wide.
$candidates = @($env:RUST_TOOLS_MSVC_ENV, 'E:\Portable\Tools\dev\msvc\devcmd.ps1', 'D:\Portable\Tools\dev\msvc\devcmd.ps1')
$envScript = $candidates | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
if (-not $env:VCToolsInstallDir -and $envScript) {
    . $envScript | Out-Null
}

if ($args[0] -eq 'dist') {
    # Each app has its own profile: see Cargo.toml.
    New-Item -ItemType Directory -Force "$PSScriptRoot\dist" | Out-Null
    foreach ($app in @(@('rcalc', 'release-small'), @('rpad', 'release-small'), @('rview', 'release'), @('rsize', 'release-small'), @('rdup', 'release-small'), @('rwall', 'release'), @('rpack', 'release'), @('runinst', 'release-small'))) {
        cargo build --profile $app[1] -p $app[0] --manifest-path "$PSScriptRoot\Cargo.toml"
        if ($LASTEXITCODE) { exit $LASTEXITCODE }
        Copy-Item "$PSScriptRoot\target\$($app[1])\$($app[0]).exe" "$PSScriptRoot\dist\"
    }
    Get-ChildItem "$PSScriptRoot\dist\*.exe" | ForEach-Object { '{0,-10} {1,10:N0} bytes' -f $_.Name, $_.Length }
    exit 0
}

cargo @args
exit $LASTEXITCODE
