# What an installation leaves on the machine, as sorted lines of text, so
# that the same steps done by Scoop and by rscoop-cli can be compared:
#
#   tree       every file, folder and link under the root (not cache, buckets
#              or Scoop itself): kind, path, size, read-only, link target or
#              number of names; the contents (SHA-256) of shims, persisted
#              data and the JSON files of the apps
#   env        the variables of the user (HKCU\Environment), not expanded
#   registry   HKCU\Software\Classes and the Run and Uninstall keys
#   shortcuts  the .lnk files in Start Menu\Programs\Scoop Apps
#   global*    the same for the global apps: their root, the variables of the
#              machine and the Start menu of all users
#   config     Scoop's settings, without last_update
#
# Runs in Windows PowerShell 5.1 (the one Windows Sandbox has).
#
# Usage: ./snapshot.ps1 -Root C:\scoop -Out C:\share\results\official\1-installed.json
param(
    [Parameter(Mandatory)][string]$Root,
    [string]$GlobalRoot = "$env:ProgramData\scoop",
    [Parameter(Mandatory)][string]$Out,
    [string]$Menu = (Join-Path ([Environment]::GetFolderPath('StartMenu')) 'Programs\Scoop Apps'),
    [string]$CommonMenu = (Join-Path ([Environment]::GetFolderPath('CommonStartMenu')) 'Programs\Scoop Apps')
)
$ErrorActionPreference = 'Stop'

if (-not ('Rscoop.Files' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
namespace Rscoop {
    public static class Files {
        // BY_HANDLE_FILE_INFORMATION: the FILETIMEs are aligned to 4 bytes.
        [StructLayout(LayoutKind.Sequential, Pack = 4)]
        struct Info {
            public uint Attributes; public long Created; public long Accessed; public long Written;
            public uint Volume; public uint SizeHigh; public uint SizeLow; public uint Links; public uint IndexHigh; public uint IndexLow;
        }
        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        static extern IntPtr CreateFileW(string name, uint access, uint share, IntPtr security, uint disposition, uint flags, IntPtr template);
        [DllImport("kernel32.dll", SetLastError = true)]
        static extern bool GetFileInformationByHandle(IntPtr handle, out Info info);
        [DllImport("kernel32.dll")]
        static extern bool CloseHandle(IntPtr handle);
        // How many names a file has (more than one: a hard link).
        public static uint Links(string path) {
            IntPtr h = CreateFileW(@"\\?\" + path, 0, 7, IntPtr.Zero, 3, 0x02000000 | 0x00200000, IntPtr.Zero);
            if (h == new IntPtr(-1)) return 0;
            Info info;
            bool ok = GetFileInformationByHandle(h, out info);
            CloseHandle(h);
            return ok ? info.Links : 0;
        }
    }
}
'@
}

$skip = '^(cache|buckets|apps\\scoop)(\\|$)'

function Get-Hash([string]$path) {
    (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.Substring(0, 16)
}

# Down the folders without going through links, into $tree, paths from $base.
function Add-Folder([string]$dir, [string]$base, $tree) {
    foreach ($path in [IO.Directory]::EnumerateFileSystemEntries($dir)) {
        $relative = $path.Substring($base.Length + 1)
        if ($relative -match $skip) { continue }
        $attributes = [IO.File]::GetAttributes($path)
        $readonly = if ($attributes -band [IO.FileAttributes]::ReadOnly) { 'ro' } else { 'rw' }
        if ($attributes -band [IO.FileAttributes]::ReparsePoint) {
            $target = @((Get-Item -LiteralPath $path -Force).Target)[0]
            $tree.Add("link|$relative|$readonly|$target")
        } elseif ($attributes -band [IO.FileAttributes]::Directory) {
            $tree.Add("dir|$relative|$readonly")
            Add-Folder $path $base $tree
        } else {
            $size = (New-Object IO.FileInfo $path).Length
            $names = [Rscoop.Files]::Links($path)
            # What is made with the time of the installation in it (compiled
            # Python, pip's launchers and its RECORD) is compared by size only.
            $timed = $relative -match '\.pyc$|\.dist-info\\RECORD$|^persist\\[^\\]+\\Scripts\\[^\\]+\.exe$'
            $contents = if (-not $timed -and ($relative -match '^(shims|persist|modules)\\' -or $relative -match '^apps\\[^\\]+\\[^\\]+\\[^\\]+\.json$')) { Get-Hash $path } else { '' }
            $tree.Add("file|$relative|$readonly|$size|names=$names|$contents")
        }
    }
}
function Get-Tree([string]$base) {
    $tree = New-Object System.Collections.Generic.List[string]
    if (Test-Path -LiteralPath $base) {
        $base = (Resolve-Path -LiteralPath $base).Path.TrimEnd('\')
        Add-Folder $base $base $tree
    }
    , $tree
}
$tree = Get-Tree $Root
$globalTree = Get-Tree $GlobalRoot

function Get-Variables([string]$path) {
    $list = New-Object System.Collections.Generic.List[string]
    $key = Get-Item -LiteralPath $path
    foreach ($name in $key.GetValueNames()) {
        $kind = $key.GetValueKind($name)
        $value = $key.GetValue($name, $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
        $list.Add("$name|$kind|$value")
    }
    , $list
}
$env = Get-Variables 'HKCU:\Environment'
$machineEnv = Get-Variables 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Environment'

$registry = New-Object System.Collections.Generic.List[string]
foreach ($base in 'HKCU:\Software\Classes', 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run', 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall') {
    if (-not (Test-Path -LiteralPath $base)) { continue }
    $keys = @(Get-Item -LiteralPath $base) + @(Get-ChildItem -LiteralPath $base -Recurse -ErrorAction SilentlyContinue)
    foreach ($k in $keys) {
        if ($k.Name -match '\\Local Settings(\\|$)') { continue }
        $registry.Add("key|$($k.Name)")
        foreach ($name in $k.GetValueNames()) {
            $registry.Add("value|$($k.Name)|$name|$($k.GetValue($name, $null, 'DoNotExpandEnvironmentNames'))")
        }
    }
}

function Get-Shortcuts([string]$folder) {
    $list = New-Object System.Collections.Generic.List[string]
    if (Test-Path -LiteralPath $folder) {
        $shell = New-Object -ComObject WScript.Shell
        foreach ($lnk in Get-ChildItem -LiteralPath $folder -Recurse -Force) {
            $relative = $lnk.FullName.Substring($folder.Length).TrimStart('\')
            if ($lnk.PSIsContainer) { $list.Add("dir|$relative"); continue }
            $s = $shell.CreateShortcut($lnk.FullName)
            $list.Add("lnk|$relative|$($s.TargetPath)|$($s.Arguments)|$($s.WorkingDirectory)|$($s.IconLocation)")
        }
    }
    , $list
}
$shortcuts = Get-Shortcuts $Menu
$commonShortcuts = Get-Shortcuts $CommonMenu

$config = New-Object System.Collections.Generic.List[string]
$configFile = Join-Path $env:USERPROFILE '.config\scoop\config.json'
if (Test-Path -LiteralPath $configFile) {
    $json = Get-Content -LiteralPath $configFile -Raw | ConvertFrom-Json
    foreach ($p in $json.PSObject.Properties | Where-Object Name -ne 'last_update') {
        $config.Add("$($p.Name)|$($p.Value | ConvertTo-Json -Compress -Depth 5)")
    }
}

$snapshot = [ordered]@{
    tree            = @($tree | Sort-Object)
    env             = @($env | Sort-Object)
    registry        = @($registry | Sort-Object)
    shortcuts       = @($shortcuts | Sort-Object)
    globaltree      = @($globalTree | Sort-Object)
    machineenv      = @($machineEnv | Sort-Object)
    globalshortcuts = @($commonShortcuts | Sort-Object)
    config          = @($config | Sort-Object)
}
New-Item -ItemType Directory -Force -Path (Split-Path $Out) | Out-Null
# One line per entry: easy to read and to compare.
$snapshot | ConvertTo-Json -Depth 3 | Set-Content -LiteralPath $Out -Encoding UTF8
Write-Host "Snapshot: $Out ($($tree.Count) + $($globalTree.Count) files and folders, $($env.Count) + $($machineEnv.Count) variables, $($registry.Count) registry lines, $($shortcuts.Count) + $($commonShortcuts.Count) shortcuts)"
