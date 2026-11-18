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
    # Each app names its own profile in apps/<app>/Cargo.toml
    # ([package.metadata.rust-tools]); release.yml reads the same line.
    New-Item -ItemType Directory -Force "$PSScriptRoot\dist" | Out-Null
    foreach ($dir in Get-ChildItem "$PSScriptRoot\apps" -Directory) {
        $app = $dir.Name
        $line = Select-String -Path "$($dir.FullName)\Cargo.toml" -Pattern '^profile\s*=\s*"([^"]+)"' | Select-Object -First 1
        if (-not $line) { Write-Error "apps/$app/Cargo.toml has no [package.metadata.rust-tools] profile"; exit 1 }
        $profile = $line.Matches[0].Groups[1].Value
        cargo build --profile $profile -p $app --manifest-path "$PSScriptRoot\Cargo.toml"
        if ($LASTEXITCODE) { exit $LASTEXITCODE }
        Copy-Item "$PSScriptRoot\target\$profile\$app.exe" "$PSScriptRoot\dist\"
    }
    Get-ChildItem "$PSScriptRoot\dist\*.exe" | ForEach-Object { '{0,-10} {1,10:N0} bytes' -f $_.Name, $_.Length }
    exit 0
}

cargo @args
exit $LASTEXITCODE
