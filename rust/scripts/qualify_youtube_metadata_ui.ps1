param(
    [string]$InstallRoot = "",
    [string]$Query = "OpenAI"
)

$ErrorActionPreference = "Stop"
if (-not $InstallRoot) {
    $InstallRoot = Join-Path $env:LOCALAPPDATA "Programs\ApricotPlayer2Beta"
}
$Executable = Join-Path $InstallRoot "ApricotPlayer2Beta.exe"
if (-not (Test-Path -LiteralPath $Executable -PathType Leaf)) {
    throw "Installed local beta was not found at $Executable"
}
if (Get-Process -Name "ApricotPlayer2Beta" -ErrorAction SilentlyContinue) {
    throw "Close ApricotPlayer 2 Beta before running the metadata UI qualification"
}

Add-Type -TypeDefinition @"
using System;
using System.Text;
using System.Runtime.InteropServices;

public static class ApricotMetadataUiNative {
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern IntPtr SendMessageW(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern IntPtr SendMessageW(IntPtr window, uint message, IntPtr wParam, string lParam);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern IntPtr SendMessageW(IntPtr window, uint message, IntPtr wParam, StringBuilder lParam);

    [DllImport("user32.dll")]
    public static extern IntPtr GetDlgItem(IntPtr window, int id);
}
"@

$WM_COMMAND = 0x0111
$WM_SETTEXT = 0x000C
$LB_GETCOUNT = 0x018B
$LB_GETCURSEL = 0x0188
$LB_GETTEXT = 0x0189
$LB_GETTEXTLEN = 0x018A
$LB_SETCURSEL = 0x0186
$CB_SETCURSEL = 0x014E
$LBN_SELCHANGE = 1
$ID_LIST = 1001
$ID_OPEN = 1002
$ID_SEARCH_EDIT = 1003
$ID_SEARCH_KIND = 1004
$ID_SEARCH = 1005

function Send-Command([IntPtr]$Window, [int]$Id, [int]$Notification = 0) {
    $Value = [IntPtr](($Notification -shl 16) -bor ($Id -band 0xFFFF))
    [void][ApricotMetadataUiNative]::SendMessageW($Window, $WM_COMMAND, $Value, [IntPtr]::Zero)
}

function Get-ListStrings([IntPtr]$List) {
    $Count = [int][ApricotMetadataUiNative]::SendMessageW($List, $LB_GETCOUNT, [IntPtr]::Zero, [IntPtr]::Zero)
    $Items = [System.Collections.Generic.List[string]]::new()
    for ($Index = 0; $Index -lt $Count; $Index++) {
        $Length = [int][ApricotMetadataUiNative]::SendMessageW($List, $LB_GETTEXTLEN, [IntPtr]$Index, [IntPtr]::Zero)
        if ($Length -lt 0) { continue }
        $Buffer = [System.Text.StringBuilder]::new($Length + 1)
        [void][ApricotMetadataUiNative]::SendMessageW($List, $LB_GETTEXT, [IntPtr]$Index, $Buffer)
        $Items.Add($Buffer.ToString())
    }
    return $Items.ToArray()
}

function Wait-Until([scriptblock]$Condition, [int]$TimeoutSeconds, [string]$Failure) {
    $Deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    do {
        $Value = & $Condition
        if ($null -ne $Value -and $Value -ne $false) { return $Value }
        Start-Sleep -Milliseconds 50
    } while ([DateTime]::UtcNow -lt $Deadline)
    throw $Failure
}

$Process = $null
try {
    $Process = Start-Process -FilePath $Executable -WorkingDirectory $InstallRoot -WindowStyle Minimized -PassThru
    $Window = Wait-Until {
        $Process.Refresh()
        if ($Process.HasExited) { throw "ApricotPlayer 2 Beta exited during startup" }
        if ($Process.MainWindowHandle -ne [IntPtr]::Zero) { return $Process.MainWindowHandle }
        return $null
    } 10 "ApricotPlayer 2 Beta did not create its main window"
    $List = [ApricotMetadataUiNative]::GetDlgItem($Window, $ID_LIST)
    $MenuItems = Get-ListStrings $List
    $SearchIndex = [Array]::FindIndex($MenuItems, [Predicate[string]]{ param($Item) $Item -like "*YouTube*" })
    if ($SearchIndex -lt 0) { throw "Search YouTube was not present in the main menu" }
    [void][ApricotMetadataUiNative]::SendMessageW($List, $LB_SETCURSEL, [IntPtr]$SearchIndex, [IntPtr]::Zero)
    Send-Command $Window $ID_OPEN

    $Edit = [ApricotMetadataUiNative]::GetDlgItem($Window, $ID_SEARCH_EDIT)
    $Kind = [ApricotMetadataUiNative]::GetDlgItem($Window, $ID_SEARCH_KIND)
    [void][ApricotMetadataUiNative]::SendMessageW($Edit, $WM_SETTEXT, [IntPtr]::Zero, $Query)
    [void][ApricotMetadataUiNative]::SendMessageW($Kind, $CB_SETCURSEL, [IntPtr]1, [IntPtr]::Zero)
    Send-Command $Window $ID_SEARCH

    $Rows = Wait-Until {
        $Current = @(Get-ListStrings $List)
        if ($Current.Count -ge 5 -and $Current[0] -like "*Uploaded unknown*") { return ,$Current }
        return $null
    } 30 "Video search did not produce five Python-compatible result rows"
    $Rows = Wait-Until {
        $Current = @(Get-ListStrings $List)
        if (@($Current[1..4] | Where-Object { $_ -notlike "*Uploaded unknown*" }).Count -gt 0) {
            return ,$Current
        }
        return $null
    } 45 "Background metadata did not update any non-focused result row"
    if ($Rows[0] -notlike "*Uploaded unknown*") {
        throw "The focused row changed while its screen-reader selection was active"
    }
    if ([int][ApricotMetadataUiNative]::SendMessageW($List, $LB_GETCURSEL, [IntPtr]::Zero, [IntPtr]::Zero) -ne 0) {
        throw "Metadata hydration moved the selected result"
    }

    [void][ApricotMetadataUiNative]::SendMessageW($List, $LB_SETCURSEL, [IntPtr]1, [IntPtr]::Zero)
    Send-Command $Window $ID_LIST $LBN_SELCHANGE
    $Updated = Wait-Until {
        $Current = @(Get-ListStrings $List)
        if ($Current.Count -ge 2 -and $Current[0] -notlike "*Uploaded unknown*") { return ,$Current }
        return $null
    } 5 "The deferred focused row was not refreshed after selection moved"
    if ([int][ApricotMetadataUiNative]::SendMessageW($List, $LB_GETCURSEL, [IntPtr]::Zero, [IntPtr]::Zero) -ne 1) {
        throw "Refreshing the deferred row changed the new selection"
    }
    Write-Output "YOUTUBE_METADATA_UI_OK"
    Write-Output "FIRST_ROW=$($Updated[0])"
    Write-Output "SECOND_ROW=$($Updated[1])"
}
finally {
    if ($null -ne $Process -and -not $Process.HasExited) {
        $Process.Kill()
        $Process.WaitForExit()
    }
}
