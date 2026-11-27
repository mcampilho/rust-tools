# Compares rscoop-cli with the official Scoop on the installation of the
# `scoop` on PATH, or of -Root. Only commands that read are run: nothing is
# changed.
#
# Usage: ./compare.ps1 -Exe ..\..\target\release-small\rscoop-cli.exe [-Root E:\Portable\scoop] [-Apps git,7zip]
param(
    [Parameter(Mandatory)][string]$Exe,
    [string[]]$Apps = @('git', '7zip'),
    [string]$Query = 'ripgrep',
    [string]$Root
)
$ErrorActionPreference = 'Stop'
$Exe = (Resolve-Path $Exe).Path
$shim = if ($Root) { Join-Path $Root 'apps\scoop\current\bin\scoop.ps1' } else { (Get-Command scoop.ps1).Source }
$installation = if ($Root) { $Root } else { Split-Path (Split-Path $shim) }
$script:failed = 0

# Scoop's commands give objects; run each in a PowerShell of its own.
function Get-Official([string]$command) {
    $json = pwsh -NoProfile -Command "& '$shim' $command 6>`$null | ForEach-Object { `$_ } | ConvertTo-Json -Depth 4 -AsArray"
    if ($json) { $json | Out-String | ConvertFrom-Json -Depth 8 } else { @() }
}

function Get-Ours([string[]]$arguments) {
    & $Exe --root $installation @arguments --json 2>$null | Out-String | ConvertFrom-Json -Depth 8
}

function Compare-Set([string]$what, $official, $ours, [scriptblock]$key) {
    $a = @($official | ForEach-Object $key | Sort-Object)
    $b = @($ours | ForEach-Object $key | Sort-Object)
    $difference = @(Compare-Object $a $b)
    if ($difference.Count -eq 0) {
        Write-Host "same       $what ($($a.Count))" -ForegroundColor Green
    } else {
        $script:failed++
        Write-Host "different  $what ($($a.Count) in Scoop, $($b.Count) here)" -ForegroundColor Red
        $difference | Select-Object -First 15 | ForEach-Object {
            $side = if ($_.SideIndicator -eq '<=') { 'Scoop' } else { 'here ' }
            Write-Host "    $side  $($_.InputObject)"
        }
    }
}

Write-Host "Installation: $installation"
Compare-Set 'list' (Get-Official 'list') (Get-Ours 'list') { "$($_.Name)|$($_.Version)|$($_.Source)|$($_.Info)" }
Compare-Set 'status --local' (Get-Official 'status --local') (Get-Ours 'status', '--local') {
    "$($_.Name)|$($_.'Installed Version')|$($_.'Latest Version')|$($_.Info)"
}
# The version is left out: Scoop's search returns it as a JSON element.
Compare-Set "search $Query" (Get-Official "search $Query") (Get-Ours 'search', $Query) { "$($_.Name)|$($_.Source)|$($_.Binaries)" }
Compare-Set 'bucket list' (Get-Official 'bucket list') (Get-Ours 'bucket', 'list') { "$($_.Name)|$($_.Source)|$($_.Manifests)" }
Compare-Set 'bucket known' (Get-Official 'bucket known') (Get-Ours 'bucket', 'known') { "$_" }
Compare-Set 'cache show' (Get-Official 'cache show') (Get-Ours 'cache', 'show') { "$($_.Name)|$($_.Version)|$($_.Length)" }
# Scoop's `shim list` gives objects: the properties are taken as they are.
$shimKey = { "$($_.Name)|$($_.Path)|$($_.Source)|$($_.Alternatives)|$($_.IsGlobal)|$($_.IsHidden)".ToLower() }
Compare-Set 'shim list' (Get-Official 'shim list | Select-Object Name,Path,Source,Alternatives,IsGlobal,IsHidden') (Get-Ours 'shim', 'list') $shimKey
foreach ($app in $Apps) {
    Compare-Set "depends $app" (Get-Official "depends $app") (Get-Ours 'depends', $app) { "$($_.Source)|$($_.Name)" }
    Compare-Set "prefix $app" (Get-Official "prefix $app") (Get-Ours 'prefix', $app) { "$_" }
    Compare-Set "info $app" @(Get-Official "info $app") @(Get-Ours 'info', $app) {
        "$($_.Name)|$($_.Version)|$($_.Source)|$($_.Website)|$($_.License)|$($_.Binaries)|$($_.Shortcuts)"
    }
}
exit $script:failed
