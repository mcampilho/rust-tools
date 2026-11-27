# The differential comparison of docs/rscoop.pt.md §7: the same steps (see
# inside.ps1) in two new Windows Sandboxes, one with Scoop doing them and one
# with rscoop-cli, then the snapshots of each step compared.
#
# Nothing on this PC is changed but the work folder (target\sandbox): the
# sandboxes download Scoop, git, the buckets and the apps from the Internet,
# and keep the downloads in target\sandbox\share\cache for the next run.
#
# Usage:
#   ./apps/rscoop-cli/sandbox/sandbox.ps1                 both sides, then the comparison
#   ./apps/rscoop-cli/sandbox/sandbox.ps1 -Side rscoop    one side again (the other's results stay)
#   ./apps/rscoop-cli/sandbox/sandbox.ps1 -CompareOnly    only the comparison
#   ./apps/rscoop-cli/sandbox/sandbox.ps1 -Prepare        the shared folder and the .wsb files, nothing started
#   ... -KeepOpen                                         the sandbox stays open (one side only)
param(
    [ValidateSet('both', 'official', 'rscoop')][string]$Side = 'both',
    [string]$Exe = 'target\release-small\rscoop-cli.exe',
    [string[]]$Apps = @('jq', 'which', 'psutils', 'gsudo', 'oh-my-posh', 'extras/notepadplusplus', 'extras/everything', 'extras/posh-git', 'extras/ditto'),
    # Updated again (--force) in step 2.
    [string[]]$Updated = @('jq', 'gsudo', 'notepadplusplus', 'everything'),
    # Removed in step 4 (the last one with --purge); the others by Scoop in step 5.
    [string[]]$First = @('notepadplusplus', 'everything', 'posh-git', 'ditto'),
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
$results = Join-Path $share 'results'
$here = $PSScriptRoot

function Start-Side([string]$name) {
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
    Write-Host "Starting the sandbox for '$name' ($wsb)..." -ForegroundColor Cyan
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

function Compare-Steps {
    $report = New-Object System.Collections.Generic.List[string]
    $differences = 0
    foreach ($step in '1-installed', '2-updated', '3-reset', '4-uninstalled', '5-official') {
        $a = "$results\official\$step.json"
        $b = "$results\rscoop\$step.json"
        if (-not (Test-Path $a) -or -not (Test-Path $b)) {
            $report.Add("missing    $step"); $differences++; continue
        }
        $official = Get-Content -LiteralPath $a -Raw | ConvertFrom-Json
        $ours = Get-Content -LiteralPath $b -Raw | ConvertFrom-Json
        foreach ($section in 'tree', 'env', 'registry', 'shortcuts') {
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
    [ordered]@{ apps = $Apps; updated = $Updated; first = $First } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $share 'plan.json')
    $sides = if ($Side -eq 'both') { 'official', 'rscoop' } else { @($Side) }
    # New versions for a new comparison; one side again keeps the other's.
    if ($Side -eq 'both' -and (Test-Path "$share\pins.json")) { Remove-Item -LiteralPath "$share\pins.json" }
    foreach ($s in $sides) { Start-Side $s }
    if ($KeepOpen -or $Prepare) { return }
}
exit (Compare-Steps)
