# Runs in Windows Sandbox (Windows PowerShell 5.1), started by sandbox.ps1.
# The same steps on a new Windows each time, with Scoop in C:\scoop; only who
# does steps 1 to 4 changes (Scoop's installer, git and the buckets are
# always Scoop's):
#
#   1-installed   `install` of the apps
#   2-updated     `update --force` of the updated apps (the same version
#                 again: the whole path of an update, the old folder kept)
#   3-reset       the same damage (current junctions, shims, shortcuts and
#                 the variables of the apps gone), then `reset *`
#   4-uninstalled `uninstall` of the first apps, the last of them with --purge
#   5-official    `uninstall` of the others by Scoop (what the other one left
#                 must be removable by Scoop)
#   6-imported    `import` of the `scoop export` made after step 1
#
# -Side official: by Scoop; -Side rscoop: by rscoop-cli, with
# RSCOOP_NATIVE=1. A snapshot (snapshot.ps1) after each step goes to
# C:\share\results\<side>, then done.txt (or failed.txt). The first side writes the commits of the buckets
# to pins.json and the second one uses them, so both install the same
# versions. The downloads are kept in C:\share\cache for the next run.
param(
    [Parameter(Mandatory)][ValidateSet('official', 'rscoop')][string]$Side,
    [string]$Share = 'C:\share'
)
$ErrorActionPreference = 'Continue'
$plan = Get-Content -LiteralPath "$Share\plan.json" -Raw | ConvertFrom-Json
$results = "$Share\results\$Side"
New-Item -ItemType Directory -Force -Path $results | Out-Null
Start-Transcript -LiteralPath "$results\transcript.txt" | Out-Null
$root = 'C:\scoop'
$env:SCOOP_CACHE = "$Share\cache"

function Save-Step([string]$name) {
    & "$Share\snapshot.ps1" -Root $root -Out "$results\$name.json"
}

function Invoke-Tool {
    # One list of words: `scoop @args` would pass a list inside the list as
    # one argument (and the program, unfolded, would not).
    $words = @($args | ForEach-Object { $_ })
    $line = $words -join ' '
    Write-Host "> $Side $line" -ForegroundColor Cyan
    if ($Side -eq 'official') {
        scoop @words
    } else {
        $env:RSCOOP_NATIVE = '1'
        & "$Share\rscoop-cli.exe" --root $root @words
        Remove-Item Env:\RSCOOP_NATIVE
    }
    "$Side $line -> $LASTEXITCODE" | Add-Content -LiteralPath "$results\codes.txt"
}

try {
    # 1. Scoop as its installer leaves it, then the apps.
    Set-ExecutionPolicy -Scope CurrentUser -ExecutionPolicy Bypass -Force
    $installer = "$env:TEMP\install-scoop.ps1"
    Invoke-RestMethod -Uri 'https://get.scoop.sh' -OutFile $installer
    & $installer -RunAsAdmin -ScoopDir $root
    $env:PATH = "$root\shims;$env:PATH"
    scoop install git
    # The installer brings `main` as a zip (there is no git yet): a clone
    # instead, as `scoop update` would make it, so that it can be pinned.
    scoop bucket rm main
    scoop bucket add main
    scoop bucket add extras
    $pins = "$Share\pins.json"
    if (Test-Path -LiteralPath $pins) {
        $pinned = Get-Content -LiteralPath $pins -Raw | ConvertFrom-Json
        foreach ($p in $pinned.PSObject.Properties) {
            git -C "$root\buckets\$($p.Name)" fetch -q origin
            git -C "$root\buckets\$($p.Name)" reset -q --hard $p.Value
        }
    } else {
        $commits = [ordered]@{}
        foreach ($b in 'main', 'extras') { $commits[$b] = [string](git -C "$root\buckets\$b" rev-parse HEAD) }
        $commits | ConvertTo-Json | Set-Content -LiteralPath $pins
    }
    Invoke-Tool install @($plan.apps)
    scoop list 6>$null | Select-Object Name, Version | Format-Table | Out-String | Set-Content -LiteralPath "$results\versions.txt"
    Save-Step '1-installed'
    scoop export 6>$null | Set-Content -LiteralPath "$results\scoopfile.json"

    # 2. The same version again, by the whole path of an update.
    Invoke-Tool update @($plan.updated) --force
    Save-Step '2-updated'

    # 3. The same damage on both sides, then reset.
    foreach ($app in Get-ChildItem -LiteralPath "$root\apps" -Directory | Where-Object Name -ne 'scoop') {
        $current = Join-Path $app.FullName 'current'
        if (Test-Path -LiteralPath $current) {
            attrib -R /L $current
            [IO.Directory]::Delete($current)
        }
        $file = Get-ChildItem -LiteralPath $app.FullName -Directory | ForEach-Object {
            Join-Path $_.FullName 'scoop-manifest.json'
            Join-Path $_.FullName 'manifest.json'
        } | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
        $manifest = if ($file) { Get-Content -LiteralPath $file -Raw | ConvertFrom-Json } else { $null }
        $set = if ($manifest.architecture.'64bit'.env_set) { $manifest.architecture.'64bit'.env_set } else { $manifest.env_set }
        if ($set) {
            foreach ($name in @($set.PSObject.Properties | ForEach-Object Name | Where-Object { $_ })) {
                [Environment]::SetEnvironmentVariable($name, $null, 'User')
            }
        }
    }
    Get-ChildItem -LiteralPath "$root\shims" -Force | Where-Object Name -notlike 'scoop*' | Remove-Item -Force
    $menu = Join-Path ([Environment]::GetFolderPath('StartMenu')) 'Programs\Scoop Apps'
    if (Test-Path -LiteralPath $menu) { Get-ChildItem -LiteralPath $menu -Force | Remove-Item -Recurse -Force }
    $path = [Environment]::GetEnvironmentVariable('PATH', 'User') -split ';' | Where-Object { $_ -and $_ -notlike "$root\apps\*" }
    [Environment]::SetEnvironmentVariable('PATH', ($path -join ';'), 'User')
    Invoke-Tool reset '*'
    Save-Step '3-reset'

    # 4. Uninstall the first apps, the last of them with its persisted data.
    $first = @($plan.first)
    if ($first.Count -gt 1) { Invoke-Tool uninstall @($first[0..($first.Count - 2)]) }
    Invoke-Tool uninstall $first[-1] --purge
    Save-Step '4-uninstalled'

    # 5. Scoop removes the rest.
    $rest = @(Get-ChildItem -LiteralPath "$root\apps" -Directory | Where-Object Name -ne 'scoop' | ForEach-Object Name)
    if ($rest) { scoop uninstall @rest }
    Save-Step '5-official'

    # 6. Everything back from the export of step 1.
    Invoke-Tool import "$results\scoopfile.json"
    Save-Step '6-imported'
    'done' | Set-Content -LiteralPath "$results\done.txt"
} catch {
    $_ | Out-String | Set-Content -LiteralPath "$results\failed.txt"
    Write-Host $_ -ForegroundColor Red
} finally {
    Stop-Transcript | Out-Null
}
# The host closes the sandbox when it sees done.txt (shutting it down from
# here makes Windows 10 say the connection was lost).
