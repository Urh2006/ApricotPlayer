param(
    [string]$InstallRoot = ""
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

if (-not $InstallRoot) {
    $InstallRoot = Join-Path $env:LOCALAPPDATA "Programs\ApricotPlayer2Beta"
}
$Executable = Join-Path $InstallRoot "ApricotPlayer2Beta.exe"
if (-not (Test-Path -LiteralPath $Executable -PathType Leaf)) {
    throw "Installed local beta was not found at $Executable"
}
if (Get-Process -Name "ApricotPlayer2Beta" -ErrorAction SilentlyContinue) {
    throw "Close ApricotPlayer 2 Beta before running the subscriptions UI qualification"
}

Add-Type -TypeDefinition @"
using System;
using System.Text;
using System.Runtime.InteropServices;

[StructLayout(LayoutKind.Sequential)]
public struct ApricotSubscriptionsRect
{
    public int Left;
    public int Top;
    public int Right;
    public int Bottom;
}

[StructLayout(LayoutKind.Sequential)]
public struct ApricotSubscriptionsGuiThreadInfo
{
    public int Size;
    public int Flags;
    public IntPtr Active;
    public IntPtr Focus;
    public IntPtr Capture;
    public IntPtr MenuOwner;
    public IntPtr MoveSize;
    public IntPtr Caret;
    public ApricotSubscriptionsRect CaretRect;
}

public static class ApricotSubscriptionsUiNative
{
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern IntPtr SendMessageW(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern IntPtr SendMessageW(IntPtr window, uint message, IntPtr wParam, StringBuilder lParam);

    [DllImport("user32.dll")]
    public static extern bool PostMessageW(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);

    [DllImport("user32.dll")]
    public static extern IntPtr GetDlgItem(IntPtr window, int id);

    [DllImport("user32.dll")]
    public static extern bool IsWindowVisible(IntPtr window);

    [DllImport("user32.dll")]
    public static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);

    [DllImport("user32.dll")]
    public static extern bool GetGUIThreadInfo(uint threadId, ref ApricotSubscriptionsGuiThreadInfo info);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern int GetClassNameW(IntPtr window, StringBuilder className, int maxCount);

    [DllImport("oleacc.dll")]
    public static extern int AccessibleObjectFromWindow(
        IntPtr window,
        uint objectId,
        ref Guid interfaceId,
        [MarshalAs(UnmanagedType.Interface)] out object accessibleObject
    );
}
"@

$WM_COMMAND = 0x0111
$WM_KEYDOWN = 0x0100
$WM_KEYUP = 0x0101
$LB_GETCOUNT = 0x018B
$LB_GETTEXT = 0x0189
$LB_GETTEXTLEN = 0x018A
$LB_SETCURSEL = 0x0186
$VK_ESCAPE = 27
$VK_TAB = 9
$ID_LIST = 1001
$ID_OPEN = 1002
$ID_BACK = 1006
$ID_REMOVE = 1016
$ID_CHECK = 1026
$ID_NEW = 1027
$ID_FILTER = 1028
$ID_SET_CATEGORY = 1029

function Wait-Until([scriptblock]$Condition, [int]$TimeoutSeconds, [string]$Failure) {
    $Deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    do {
        $Value = & $Condition
        if ($null -ne $Value -and $Value -ne $false) { return $Value }
        Start-Sleep -Milliseconds 20
    } while ([DateTime]::UtcNow -lt $Deadline)
    throw $Failure
}

function Get-ListStrings([IntPtr]$List) {
    $Count = [int][ApricotSubscriptionsUiNative]::SendMessageW($List, $LB_GETCOUNT, [IntPtr]::Zero, [IntPtr]::Zero)
    $Items = [System.Collections.Generic.List[string]]::new()
    for ($Index = 0; $Index -lt $Count; $Index++) {
        $Length = [int][ApricotSubscriptionsUiNative]::SendMessageW($List, $LB_GETTEXTLEN, [IntPtr]$Index, [IntPtr]::Zero)
        if ($Length -lt 0) { continue }
        $Buffer = [System.Text.StringBuilder]::new($Length + 1)
        [void][ApricotSubscriptionsUiNative]::SendMessageW($List, $LB_GETTEXT, [IntPtr]$Index, $Buffer)
        $Items.Add($Buffer.ToString())
    }
    return $Items.ToArray()
}

function Get-FocusedHandle([IntPtr]$Window) {
    [uint32]$ProcessId = 0
    $ThreadId = [ApricotSubscriptionsUiNative]::GetWindowThreadProcessId($Window, [ref]$ProcessId)
    $Info = [ApricotSubscriptionsGuiThreadInfo]::new()
    $Info.Size = [System.Runtime.InteropServices.Marshal]::SizeOf([type][ApricotSubscriptionsGuiThreadInfo])
    if ($ThreadId -eq 0 -or -not [ApricotSubscriptionsUiNative]::GetGUIThreadInfo($ThreadId, [ref]$Info)) {
        throw "Could not inspect application focus"
    }
    return $Info.Focus
}

function Get-MsaaElement([IntPtr]$Window) {
    $InterfaceId = [Guid]"618736e0-3c3d-11cf-810c-00aa00389b71"
    $AccessibleObject = $null
    $Result = [ApricotSubscriptionsUiNative]::AccessibleObjectFromWindow(
        $Window,
        [uint32]4294967292,
        [ref]$InterfaceId,
        [ref]$AccessibleObject
    )
    if ($Result -ne 0 -or $null -eq $AccessibleObject) {
        throw "Could not read control through MSAA (HRESULT $Result)"
    }
    $Flags = [System.Reflection.BindingFlags]::GetProperty
    return [pscustomobject]@{
        Name = $AccessibleObject.GetType().InvokeMember("accName", $Flags, $null, $AccessibleObject, @([object]0))
        Role = [int]$AccessibleObject.GetType().InvokeMember("accRole", $Flags, $null, $AccessibleObject, @([object]0))
    }
}

function Get-ControlClass([IntPtr]$Window) {
    $ClassName = [System.Text.StringBuilder]::new(64)
    [void][ApricotSubscriptionsUiNative]::GetClassNameW($Window, $ClassName, $ClassName.Capacity)
    return $ClassName.ToString()
}

function Send-Key([IntPtr]$Window, [int]$Key) {
    [void][ApricotSubscriptionsUiNative]::PostMessageW($Window, $WM_KEYDOWN, [IntPtr]$Key, [IntPtr]::Zero)
    [void][ApricotSubscriptionsUiNative]::PostMessageW($Window, $WM_KEYUP, [IntPtr]$Key, [IntPtr]::Zero)
    Start-Sleep -Milliseconds 50
}

$TemporaryRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("apricot-subscriptions-ui-" + [Guid]::NewGuid().ToString("N"))
$Process = $null
try {
    $Roaming = Join-Path $TemporaryRoot "Roaming"
    $Data = Join-Path $Roaming "ApricotPlayer2Beta"
    New-Item -ItemType Directory -Path $Data -Force | Out-Null
    $SourceSettings = Join-Path $env:APPDATA "ApricotPlayer2Beta\settings.json"
    if (-not (Test-Path -LiteralPath $SourceSettings -PathType Leaf)) {
        throw "A complete beta settings template was not found at $SourceSettings"
    }
    $Settings = Get-Content -LiteralPath $SourceSettings -Raw | ConvertFrom-Json
    $Settings.language = "en"
    $Settings.language_prompted = $true
    $Settings.subscription_check_enabled = $false
    $Settings.main_menu_hidden_actions = @($Settings.main_menu_hidden_actions | Where-Object { $_ -ne "subscriptions" })
    [System.IO.File]::WriteAllText(
        (Join-Path $Data "settings.json"),
        ($Settings | ConvertTo-Json -Depth 20),
        [System.Text.UTF8Encoding]::new($false)
    )
    $Subscriptions = @(
        [ordered]@{
            title = "Alpha Channel"
            url = "https://www.youtube.com/@alpha"
            category = "Music"
            latest_urls = @()
            last_checked = 0
            last_new_count = 0
            last_new_items = @()
            created_at = 1
        },
        [ordered]@{
            title = "Beta Channel"
            url = "https://www.youtube.com/@beta"
            latest_urls = @("https://www.youtube.com/watch?v=abcdefghijk")
            last_checked = 0
            last_new_count = 2
            last_new_items = @()
            created_at = 2
        }
    )
    [System.IO.File]::WriteAllText(
        (Join-Path $Data "subscriptions.json"),
        ($Subscriptions | ConvertTo-Json -Depth 20),
        [System.Text.UTF8Encoding]::new($false)
    )

    $OriginalAppData = $env:APPDATA
    try {
        $env:APPDATA = $Roaming
        $Process = Start-Process -FilePath $Executable -WorkingDirectory $InstallRoot -PassThru
    } finally {
        $env:APPDATA = $OriginalAppData
    }
    $Window = Wait-Until {
        $Process.Refresh()
        if ($Process.HasExited) { throw "ApricotPlayer 2 Beta exited during startup" }
        if ($Process.MainWindowHandle -ne [IntPtr]::Zero) { return $Process.MainWindowHandle }
        return $null
    } 10 "ApricotPlayer 2 Beta did not create its main window"
    $List = [ApricotSubscriptionsUiNative]::GetDlgItem($Window, $ID_LIST)
    $MenuItems = @(Get-ListStrings $List)
    $Index = [Array]::FindIndex($MenuItems, [Predicate[string]]{ param($Item) $Item -match "^Subscriptions" })
    if ($Index -lt 0) { throw "Subscriptions was not present in the main menu" }
    [void][ApricotSubscriptionsUiNative]::SendMessageW($List, $LB_SETCURSEL, [IntPtr]$Index, [IntPtr]::Zero)
    [void][ApricotSubscriptionsUiNative]::PostMessageW($Window, $WM_COMMAND, [IntPtr]$ID_OPEN, [IntPtr]::Zero)

    $Controls = @(
        [pscustomobject]@{ Id = $ID_LIST; Name = "Subscriptions"; Class = "ListBox"; Role = 33 },
        [pscustomobject]@{ Id = $ID_BACK; Name = "Back to main menu"; Class = "Button"; Role = 43 },
        [pscustomobject]@{ Id = $ID_CHECK; Name = "Check subscriptions now"; Class = "Button"; Role = 43 },
        [pscustomobject]@{ Id = $ID_OPEN; Name = "Open channel videos"; Class = "Button"; Role = 43 },
        [pscustomobject]@{ Id = $ID_NEW; Name = "New videos"; Class = "Button"; Role = 43 },
        [pscustomobject]@{ Id = $ID_REMOVE; Name = "Remove"; Class = "Button"; Role = 43 },
        [pscustomobject]@{ Id = $ID_FILTER; Name = "Filter by category"; Class = "Button"; Role = 43 },
        [pscustomobject]@{ Id = $ID_SET_CATEGORY; Name = "Set category"; Class = "Button"; Role = 43 }
    )
    foreach ($Expected in $Controls) {
        $Control = [ApricotSubscriptionsUiNative]::GetDlgItem($Window, $Expected.Id)
        if (-not [ApricotSubscriptionsUiNative]::IsWindowVisible($Control)) {
            throw "$($Expected.Name) was not visible"
        }
        $Msaa = Get-MsaaElement $Control
        $Class = Get-ControlClass $Control
        if ($Msaa.Name -ne $Expected.Name -or $Msaa.Role -ne $Expected.Role -or $Class -ne $Expected.Class) {
            throw "$($Expected.Name) exposed '$($Msaa.Name)', role $($Msaa.Role), class $Class"
        }
    }

    $Rows = @(Get-ListStrings $List)
    if ($Rows.Count -ne 2 -or $Rows[0] -notmatch "2 new videos from Beta Channel" -or $Rows[1] -notmatch "Alpha Channel \| Category: Music \| never checked") {
        throw "Subscription rows did not match the Python field order: $($Rows -join ' / ')"
    }

    $FocusPath = [System.Collections.Generic.List[string]]::new()
    for ($Step = 0; $Step -lt 8; $Step++) {
        $FocusPath.Add((Get-MsaaElement (Get-FocusedHandle $Window)).Name)
        Send-Key (Get-FocusedHandle $Window) $VK_TAB
    }
    $ExpectedPath = @(
        "Subscriptions",
        "Back to main menu",
        "Check subscriptions now",
        "Open channel videos",
        "New videos",
        "Remove",
        "Filter by category",
        "Set category"
    )
    if (Compare-Object -ReferenceObject $ExpectedPath -DifferenceObject $FocusPath.ToArray() -SyncWindow 0) {
        throw "Unexpected subscriptions Tab path: $($FocusPath -join ' > ')"
    }
    Write-Output "SUBSCRIPTIONS_ACCESSIBILITY_UI=PASS"
    Write-Output "SUBSCRIPTIONS_TAB_PATH=$($FocusPath -join ' > ')"

    Send-Key (Get-FocusedHandle $Window) $VK_ESCAPE
    $ReturnedItems = @(Get-ListStrings $List)
    if (-not ($ReturnedItems -match "^Subscriptions")) {
        throw "Escape did not return from Subscriptions to the main menu"
    }
    Write-Output "SUBSCRIPTIONS_ESCAPE_RETURN=PASS"
    Write-Output "YOUTUBE_SUBSCRIPTIONS_UI_OK"
}
finally {
    if ($null -ne $Process -and -not $Process.HasExited) {
        $Process.Kill()
        $Process.WaitForExit()
    }
    if (Test-Path -LiteralPath $TemporaryRoot) {
        Remove-Item -LiteralPath $TemporaryRoot -Recurse -Force
    }
}
