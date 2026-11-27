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
#
# Runs in Windows PowerShell 5.1 (the one Windows Sandbox has).
#
# Usage: ./snapshot.ps1 -Root C:\scoop -Out C:\share\results\official\1-installed.json
param(
    [Parameter(Mandatory)][string]$Root,
    [Parameter(Mandatory)][string]$Out,
    [string]$Menu = (Join-Path ([Environment]::GetFolderPath('StartMenu')) 'Programs\Scoop Apps')
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

$Root = (Resolve-Path -LiteralPath $Root).Path.TrimEnd('\')
$skip = '^(cache|buckets|apps\\scoop)(\\|$)'
$tree = New-Object System.Collections.Generic.List[string]

function Get-Hash([string]$path) {
    (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.Substring(0, 16)
}

# Down the folders without going through links.
function Add-Folder([string]$dir) {
    foreach ($path in [IO.Directory]::EnumerateFileSystemEntries($dir)) {
        $relative = $path.Substring($Root.Length + 1)
        if ($relative -match $skip) { continue }
        $attributes = [IO.File]::GetAttributes($path)
        $readonly = if ($attributes -band [IO.FileAttributes]::ReadOnly) { 'ro' } else { 'rw' }
        if ($attributes -band [IO.FileAttributes]::ReparsePoint) {
            $target = @((Get-Item -LiteralPath $path -Force).Target)[0]
            $tree.Add("link|$relative|$readonly|$target")
        } elseif ($attributes -band [IO.FileAttributes]::Directory) {
            $tree.Add("dir|$relative|$readonly")
            Add-Folder $path
        } else {
            $size = (New-Object IO.FileInfo $path).Length
            $names = [Rscoop.Files]::Links($path)
            $contents = if ($relative -match '^(shims|persist|modules)\\' -or $relative -match '^apps\\[^\\]+\\[^\\]+\\[^\\]+\.json$') { Get-Hash $path } else { '' }
            $tree.Add("file|$relative|$readonly|$size|names=$names|$contents")
        }
    }
}
Add-Folder $Root

$env = New-Object System.Collections.Generic.List[string]
$key = Get-Item -LiteralPath 'HKCU:\Environment'
foreach ($name in $key.GetValueNames()) {
    $kind = $key.GetValueKind($name)
    $value = $key.GetValue($name, $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
    $env.Add("$name|$kind|$value")
}

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

$shortcuts = New-Object System.Collections.Generic.List[string]
if (Test-Path -LiteralPath $Menu) {
    $shell = New-Object -ComObject WScript.Shell
    foreach ($lnk in Get-ChildItem -LiteralPath $Menu -Recurse -Force) {
        $relative = $lnk.FullName.Substring($Menu.Length).TrimStart('\')
        if ($lnk.PSIsContainer) { $shortcuts.Add("dir|$relative"); continue }
        $s = $shell.CreateShortcut($lnk.FullName)
        $shortcuts.Add("lnk|$relative|$($s.TargetPath)|$($s.Arguments)|$($s.WorkingDirectory)|$($s.IconLocation)")
    }
}

$snapshot = [ordered]@{
    tree      = @($tree | Sort-Object)
    env       = @($env | Sort-Object)
    registry  = @($registry | Sort-Object)
    shortcuts = @($shortcuts | Sort-Object)
}
New-Item -ItemType Directory -Force -Path (Split-Path $Out) | Out-Null
# One line per entry: easy to read and to compare.
$snapshot | ConvertTo-Json -Depth 3 | Set-Content -LiteralPath $Out -Encoding UTF8
Write-Host "Snapshot: $Out ($($tree.Count) files and folders, $($env.Count) variables, $($registry.Count) registry lines, $($shortcuts.Count) shortcuts)"
