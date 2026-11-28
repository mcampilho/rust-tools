# The differential comparison of docs/rscoop.pt.md §7: the same steps (see
# inside.ps1) in two new Windows Sandboxes, one with Scoop doing them and one
# with rscoop-cli, then the snapshots of each step compared.
#
# Nothing on this PC is changed but the work folder (target\sandbox): the
# sandboxes download Scoop, git, the buckets and the apps from the Internet,
# and keep the downloads in target\sandbox\share\cache for the next run.
#
# The scenarios (-Scenario, several or 'all'):
#   default   user apps, the default settings, and an app@version
#   global    the apps installed with --global (C:\ProgramData\scoop, the
#             variables of the machine, the Start menu of all users)
#   settings  no_junction and use_isolated_path on
#   heavy     apps with installers and scripts of their own
#
# Usage:
#   ./apps/rscoop-cli/sandbox/sandbox.ps1                       the default scenario, both sides, then the comparison
#   ./apps/rscoop-cli/sandbox/sandbox.ps1 -Scenario all         every scenario
#   ./apps/rscoop-cli/sandbox/sandbox.ps1 -Scenario global -Side rscoop   one side again (the other's results stay)
#   ./apps/rscoop-cli/sandbox/sandbox.ps1 -CompareOnly          only the comparison (of the scenarios asked)
#   ./apps/rscoop-cli/sandbox/sandbox.ps1 -Prepare              the shared folder and the .wsb files, nothing started
#   ... -KeepOpen                                               the sandbox stays open (one side only)
param(
    [ValidateSet('default', 'global', 'settings', 'heavy', 'all')][string[]]$Scenario = @('default'),
    [ValidateSet('both', 'official', 'rscoop')][string]$Side = 'both',
    [string]$Exe = 'target\release-small\rscoop-cli.exe',
    [string]$Work = 'target\sandbox',
    [int]$Minutes = 90,
    [switch]$CompareOnly,
    [switch]$Prepare,
    [switch]$KeepOpen
)
$ErrorActionPreference = 'Stop'
# -Exe and -Work count from the root of the repository, wherever this runs from.
$repository = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..\..'))
$Work = [IO.Path]::GetFullPath([IO.Path]::Combine($repository, $Work))
$Exe = [IO.Path]::GetFullPath([IO.Path]::Combine($repository, $Exe))
$share = Join-Path $Work 'share'
$here = $PSScriptRoot

# apps: installed in step 1; updated: again (--force) in step 2; first:
# removed in step 4 (the last one with --purge), the others by Scoop in
# step 5; version: installed in step 7 (an app the import does not bring).
$plans = [ordered]@{
    default  = [ordered]@{
        apps    = @('jq', 'which', 'psutils', 'gsudo', 'oh-my-posh', 'extras/notepadplusplus', 'extras/everything', 'extras/posh-git', 'extras/ditto')
        updated = @('jq', 'gsudo', 'notepadplusplus', 'everything')
        first   = @('notepadplusplus', 'everything', 'posh-git', 'ditto')
        version = 'fzf@0.74.3'
    }
    global   = [ordered]@{
        apps    = @('jq', 'psutils', 'gsudo', 'extras/notepadplusplus', 'extras/posh-git')
        updated = @('jq', 'notepadplusplus')
        first   = @('notepadplusplus', 'posh-git')
        global  = $true
    }
    settings = [ordered]@{
        apps     = @('jq', 'gsudo', 'oh-my-posh', 'extras/notepadplusplus', 'extras/ditto')
        updated  = @('jq', 'notepadplusplus')
        first    = @('notepadplusplus', 'ditto')
        settings = [ordered]@{ no_junction = 'true'; use_isolated_path = 'true' }
    }
    heavy    = [ordered]@{
        apps    = @('python', 'extras/vscode', 'extras/windows-terminal')
        updated = @('python')
        first   = @('vscode', 'windows-terminal')
    }
}
$scenarios = if ($Scenario -contains 'all') { @($plans.Keys) } else { $Scenario }

function Start-Side([string]$scenario, [string]$name) {
    $results = Join-Path $share "results\$scenario"
    $plan = $plans[$scenario]
    $plan['scenario'] = $scenario
    $plan | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $share 'plan.json')
    $wsb = Join-Path $Work "$name.wsb"
    $command = "powershell.exe -ExecutionPolicy Bypass -Command `"Start-Process powershell.exe -ArgumentList '-NoExit','-NoProfile','-ExecutionPolicy','Bypass','-File','C:\share\inside.ps1','-Side','$name'`""
    @"
<Configuration>
  <MappedFolders>
    <MappedFolder>
      <HostFolder>$share</HostFolder>
      <SandboxFolder>C:\share</SandboxFolder>
      <ReadOnly>false</ReadOnly>
    </MappedFolder>
  </MappedFolders>
  <MemoryInMB>4096</MemoryInMB>
  <LogonCommand>
    <Command>$([Security.SecurityElement]::Escape($command))</Command>
  </LogonCommand>
</Configuration>
"@ | Set-Content -LiteralPath $wsb -Encoding UTF8
    if ($Prepare) { Write-Host "Ready: $wsb"; return }
    $out = Join-Path $results $name
    if (Test-Path -LiteralPath $out) { Remove-Item -LiteralPath $out -Recurse -Force }
    Write-Host "Starting the sandbox for '$scenario', '$name' ($wsb)..." -ForegroundColor Cyan
    Start-Process -FilePath "$env:SystemRoot\System32\WindowsSandbox.exe" -ArgumentList "`"$wsb`""
    if ($KeepOpen) { return }
    $deadline = (Get-Date).AddMinutes($Minutes)
    while (-not (Test-Path "$out\done.txt") -and -not (Test-Path "$out\failed.txt")) {
        if ((Get-Date) -gt $deadline) { throw "The sandbox for '$name' did not finish in $Minutes minutes: see $out\transcript.txt" }
        Start-Sleep -Seconds 20
    }
    # A sandbox where something failed stays open, to be looked at.
    if (Test-Path "$out\failed.txt") { throw "The steps failed in the sandbox for '$name' (it is still open): $(Get-Content -Raw "$out\failed.txt")" }
    # Only one sandbox at a time: this one is closed from here (shut down
    # from inside, Windows 10 says the connection was lost).
    Get-Process -Name 'WindowsSandboxClient', 'WindowsSandbox' -ErrorAction SilentlyContinue | Stop-Process -Force
    $deadline = (Get-Date).AddMinutes(5)
    while (Get-Process -Name 'WindowsSandbox', 'WindowsSandboxClient' -ErrorAction SilentlyContinue) {
        if ((Get-Date) -gt $deadline) { throw 'The sandbox did not close: close it and run again with -Side or -CompareOnly.' }
        Start-Sleep -Seconds 5
    }
    Start-Sleep -Seconds 10
}

function Compare-Steps([string]$scenario, $report) {
    $results = Join-Path $share "results\$scenario"
    $differences = 0
    $steps = @(Get-ChildItem -LiteralPath "$results\official" -Filter '*.json' -ErrorAction SilentlyContinue | Where-Object Name -match '^\d-' | ForEach-Object BaseName | Sort-Object)
    if (-not $steps) { $report.Add("missing    $scenario"); return 1 }
    $report.Add("== $scenario")
    foreach ($step in $steps) {
        $a = "$results\official\$step.json"
        $b = "$results\rscoop\$step.json"
        if (-not (Test-Path $a) -or -not (Test-Path $b)) {
            $report.Add("missing    $step"); $differences++; continue
        }
        $official = Get-Content -LiteralPath $a -Raw | ConvertFrom-Json
        $ours = Get-Content -LiteralPath $b -Raw | ConvertFrom-Json
        foreach ($section in 'tree', 'env', 'registry', 'shortcuts', 'globaltree', 'machineenv', 'globalshortcuts', 'config') {
            $diff = @(Compare-Object @($official.$section) @($ours.$section))
            if ($diff.Count -eq 0) {
                $report.Add("same       $step $section ($(@($official.$section).Count))")
            } else {
                $differences++
                $report.Add("different  $step $section ($($diff.Count) lines)")
                foreach ($d in $diff | Select-Object -First 40) {
                    $who = if ($d.SideIndicator -eq '<=') { 'Scoop    ' } else { 'rscoop   ' }
                    $report.Add("    $who $($d.InputObject)")
                }
            }
        }
    }
    foreach ($s in 'official', 'rscoop') {
        if (Test-Path "$results\$s\codes.txt") { $report.Add("exit codes, ${s}: $((Get-Content "$results\$s\codes.txt") -join '; ')") }
    }
    return $differences
}

function Compare-All {
    $report = New-Object System.Collections.Generic.List[string]
    $differences = 0
    foreach ($s in $scenarios) { $differences += Compare-Steps $s $report }
    $report | Set-Content -LiteralPath (Join-Path $Work 'report.txt') -Encoding UTF8
    foreach ($line in $report) {
        $color = if ($line -like 'same*') { 'Green' } elseif ($line -like 'different*' -or $line -like 'missing*') { 'Red' } else { 'Gray' }
        Write-Host $line -ForegroundColor $color
    }
    Write-Host "Report: $(Join-Path $Work 'report.txt')"
    return $differences
}

if (-not $CompareOnly) {
    if (-not (Test-Path "$env:SystemRoot\System32\WindowsSandbox.exe")) { throw 'Windows Sandbox is not enabled on this PC.' }
    if ($KeepOpen -and $Side -eq 'both') { throw '-KeepOpen runs one side: add -Side official or -Side rscoop.' }
    New-Item -ItemType Directory -Force -Path $share, (Join-Path $share 'cache') | Out-Null
    Copy-Item -LiteralPath $Exe -Destination (Join-Path $share 'rscoop-cli.exe') -Force
    Copy-Item -LiteralPath (Join-Path $here 'inside.ps1'), (Join-Path $here 'snapshot.ps1') -Destination $share -Force
    $sides = if ($Side -eq 'both') { 'official', 'rscoop' } else { @($Side) }
    # New versions for a new comparison; one side again keeps the other's.
    if ($Side -eq 'both' -and -not $Prepare -and (Test-Path "$share\pins.json")) { Remove-Item -LiteralPath "$share\pins.json" }
    foreach ($sc in $scenarios) {
        foreach ($s in $sides) { Start-Side $sc $s }
    }
    if ($KeepOpen -or $Prepare) { return }
}
exit (Compare-All)
