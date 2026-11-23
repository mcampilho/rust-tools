# Releases on GitHub: the patch of each app goes up (x.y.z -> x.y.z+1, never the
# major or minor), one commit, a push of master, and the tags pushed three at a
# time (GitHub starts the workflows of no more than three tags of one push).
# Then, once the releases are published: ./dev.ps1 dist (the local builds) and
# checkver.ps1 -Dir bucket -Update (the Scoop bucket).
#
# Usage: ./release.ps1                 the apps changed since their last release
#        ./release.ps1 -Apps rcare,rpad these apps
#        ./release.ps1 -All             every app that has releases
#        ./release.ps1 -DryRun          only shows what it would do
#        ./release.ps1 -Wait            waits for each group's releases before the next
#        ./release.ps1 -Yes             does not ask before changing anything
#
# An app whose version was already raised by hand (newer than its last tag), or
# that never had a release, is released with the version it has. The apps that
# have releases are the ones release.yml lists in its tags.
param(
    [string[]]$Apps,
    [switch]$All,
    [switch]$DryRun,
    [switch]$Wait,
    [switch]$Yes
)
$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot

function Fail([string]$why) { Write-Host "release.ps1: $why" -ForegroundColor Red; exit 1 }

function Invoke-Git {
    # git writes its progress to stderr: in Windows PowerShell that is not an error by itself, only the exit code is.
    $ErrorActionPreference = 'Continue'
    $out = & git @args 2>&1 | ForEach-Object { "$_" }
    if ($LASTEXITCODE) { Fail "git $($args -join ' '): $($out | Out-String)" }
    $out
}

# The apps that have releases: release.yml's tags ("rcalc-v*", …).
$yml = Get-Content -Raw "$PSScriptRoot\.github\workflows\release.yml"
$m = [regex]::Match($yml, 'tags:\s*\[([^\]]+)\]')
if (-not $m.Success) { Fail 'release.yml has no tags list' }
$releasable = [regex]::Matches($m.Groups[1].Value, '"([a-z0-9-]+)-v\*"') | ForEach-Object { $_.Groups[1].Value }

# Parses "x.y.z" (and compares).
function ConvertTo-Version([string]$v) { [version]$v }

function Get-AppVersion([string]$app) {
    $toml = "$PSScriptRoot\apps\$app\Cargo.toml"
    if (-not (Test-Path $toml)) { Fail "apps/$app/Cargo.toml does not exist" }
    # The first `version = "…"` is [package]'s (it comes before the dependencies).
    $line = Select-String -Path $toml -Pattern '^version\s*=\s*"(\d+)\.(\d+)\.(\d+)"' | Select-Object -First 1
    if (-not $line) { Fail "apps/$app/Cargo.toml has no version" }
    $g = $line.Matches[0].Groups
    "$($g[1].Value).$($g[2].Value).$($g[3].Value)"
}

function Set-AppVersion([string]$app, [string]$version) {
    $toml = "$PSScriptRoot\apps\$app\Cargo.toml"
    $text = [IO.File]::ReadAllText($toml)
    $re = [regex]'(?m)^version\s*=\s*"\d+\.\d+\.\d+"'
    $text = $re.Replace($text, "version = `"$version`"", 1)
    [IO.File]::WriteAllText($toml, $text, (New-Object Text.UTF8Encoding($false)))
}

# The newest tag of an app, by version (or $null).
function Get-LastTag([string]$app) {
    $tags = & git tag --list "$app-v*" 2>$null | Where-Object { $_ -match "^$([regex]::Escape($app))-v\d+\.\d+\.\d+$" }
    $tags | Sort-Object { ConvertTo-Version ($_ -replace '^.*-v', '') } -Descending | Select-Object -First 1
}

# --- Where things stand.
$branch = (Invoke-Git rev-parse --abbrev-ref HEAD | Out-String).Trim()
if ($branch -ne 'master') { Fail "on branch '$branch', not master" }
if ((Invoke-Git status --porcelain | Out-String).Trim()) {
    if (-not $DryRun) { Fail 'the working tree has changes: commit or stash them first' }
    Write-Host 'The working tree has changes (a real run would stop here).' -ForegroundColor Yellow
}
Invoke-Git fetch --quiet --tags origin | Out-Null
$behind = [int]((Invoke-Git rev-list --count 'HEAD..origin/master' | Out-String).Trim())
if ($behind -gt 0) { Fail "master is $behind commits behind origin/master: pull first" }

# --- Which apps.
if ($Apps) {
    $chosen = $Apps | ForEach-Object { $_ -split ',' } | ForEach-Object { $_.Trim() } | Where-Object { $_ }
    foreach ($a in $chosen) { if ($releasable -notcontains $a) { Fail "'$a' has no releases (not in release.yml's tags)" } }
} else {
    $chosen = $releasable
}

$plan = @()
$unchanged = @()
foreach ($app in $chosen) {
    $current = Get-AppVersion $app
    $last = Get-LastTag $app
    $lastVersion = if ($last) { $last -replace '^.*-v', '' } else { $null }
    # What changed since: the app's folder and the crates it is built from, but documentation
    # (a README or a CLAUDE.md is not a reason for a new release).
    $changes = 0
    if ($last) {
        $changes = [int]((Invoke-Git rev-list --count "$last..HEAD" -- "apps/$app" crates ':(exclude)*.md' | Out-String).Trim())
    }
    if (-not $All -and -not $Apps -and $last -and $changes -eq 0) {
        $unchanged += "$app $($last -replace '^.*-v', '')"
        continue
    }
    if (-not $last) {
        $new = $current; $why = 'first release'
    } elseif ((ConvertTo-Version $current) -gt (ConvertTo-Version $lastVersion)) {
        $new = $current; $why = 'raised by hand'
    } else {
        $v = ConvertTo-Version $lastVersion
        $new = "$($v.Major).$($v.Minor).$($v.Build + 1)"; $why = 'patch'
    }
    if (& git tag --list "$app-v$new") { Fail "the tag $app-v$new exists already" }
    $plan += [pscustomobject]@{ App = $app; Last = $(if ($last) { $last } else { '-' }); Current = $current; New = $new; Why = $why; Changes = $changes }
}

# The apps left out: nothing changed since their last release (documentation apart).
$same = if ($unchanged) { "Unchanged since their last release (-Apps or -All to release them too): $($unchanged -join ', ')." }
if (-not $plan) {
    Write-Host 'Nothing to release: no app changed since its last release.'
    if ($same) { Write-Host $same }
    exit 0
}

$plan | Format-Table App, Last, Current, New, Why, Changes -AutoSize | Out-String | Write-Host
if ($same) { Write-Host $same }
$groups = [math]::Ceiling($plan.Count / 3)
Write-Host "$($plan.Count) releases: one commit, a push of master, then the tags in $groups push(es) of up to three."
if ($DryRun) { Write-Host 'Dry run: nothing was changed.'; exit 0 }
if (-not $Yes) {
    $answer = Read-Host 'Go on? (y/N)'
    if ($answer -notmatch '^(y|yes|s|sim)$') { Write-Host 'Nothing was changed.'; exit 0 }
}

# --- The versions, the lock file, the commit.
foreach ($p in $plan) {
    if ($p.New -ne $p.Current) { Set-AppVersion $p.App $p.New }
}
& "$PSScriptRoot\dev.ps1" update --workspace --offline
if ($LASTEXITCODE) { Fail 'cargo update --workspace did not work: the versions are changed but nothing was committed' }
$changed = (Invoke-Git status --porcelain | Out-String).Trim()
if ($changed) {
    Invoke-Git add -- Cargo.lock ($plan | ForEach-Object { "apps/$($_.App)/Cargo.toml" }) | Out-Null
    $title = 'release: ' + (($plan | ForEach-Object { "$($_.App) $($_.New)" }) -join ', ')
    Invoke-Git commit -q -m $title | Out-Null
    Write-Host "Committed: $title"
}
Invoke-Git push -q origin master | Out-Null
Write-Host 'Pushed master.'

# --- The tags, three a push.
$tags = @($plan | ForEach-Object { "$($_.App)-v$($_.New)" })
foreach ($t in $tags) { Invoke-Git tag -a $t -m ($t -replace '-v', ' ') | Out-Null }
for ($i = 0; $i -lt $tags.Count; $i += 3) {
    $group = @($tags[$i..([math]::Min($i + 2, $tags.Count - 1))])
    Invoke-Git push -q origin @group | Out-Null
    Write-Host "Pushed: $($group -join ', ')"
    $last = ($i + 3) -ge $tags.Count
    if ($Wait) {
        # Each tag's release run, waited for (they show up a few seconds after the push).
        Start-Sleep -Seconds 15
        foreach ($t in $group) {
            $run = gh run list --workflow release.yml --branch $t --limit 1 --json databaseId | ConvertFrom-Json
            if ($run) {
                gh run watch $run[0].databaseId --exit-status --interval 30 | Out-Null
                $state = if ($LASTEXITCODE) { 'FAILED' } else { 'published' }
                Write-Host "  $t`: $state"
            } else {
                Write-Host "  $t`: no run found (gh run list --workflow release.yml)"
            }
        }
    } elseif (-not $last) {
        # A pause between pushes, so each is a push of its own for GitHub.
        Start-Sleep -Seconds 20
    }
}

Write-Host ''
Write-Host 'When the releases are published (gh run list --workflow release.yml):'
Write-Host '  ./dev.ps1 dist                              the local builds, in dist\'
Write-Host '  checkver.ps1 -Dir bucket -Update            the Scoop bucket (then commit it)'
