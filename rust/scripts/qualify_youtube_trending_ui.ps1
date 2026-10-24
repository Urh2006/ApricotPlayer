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
    throw "Close ApricotPlayer 2 Beta before running the Trending UI qualification"
}

Add-Type -TypeDefinition @"
using System;
using System.Text;
using System.Runtime.InteropServices;

[StructLayout(LayoutKind.Sequential)]
public struct ApricotTrendingRect
{
    public int Left;
    public int Top;
    public int Right;
    public int Bottom;
}

[StructLayout(LayoutKind.Sequential)]
public struct ApricotTrendingGuiThreadInfo
{
    public int Size;
    public int Flags;
    public IntPtr Active;
    public IntPtr Focus;
    public IntPtr Capture;
    public IntPtr MenuOwner;
    public IntPtr MoveSize;
    public IntPtr Caret;
    public ApricotTrendingRect CaretRect;
}

public static class ApricotTrendingUiNative
{
    public delegate bool EnumWindowsProc(IntPtr window, IntPtr parameter);

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
    public static extern bool GetGUIThreadInfo(uint threadId, ref ApricotTrendingGuiThreadInfo info);

    [DllImport("user32.dll")]
    public static extern bool EnumWindows(EnumWindowsProc callback, IntPtr parameter);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern int GetClassNameW(IntPtr window, StringBuilder className, int maxCount);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern int GetWindowTextW(IntPtr window, StringBuilder text, int maxCount);

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
$CB_GETCOUNT = 0x0146
$CB_GETCURSEL = 0x0147
$VK_TAB = 9
$VK_RETURN = 13
$ID_LIST = 1001
$ID_OPEN = 1002
$ID_TRENDING_COUNTRY = 1023
$ID_TRENDING_CATEGORY = 1024
$ID_LOAD_TRENDING = 1025

function Get-ListStrings([IntPtr]$List) {
    $Count = [int][ApricotTrendingUiNative]::SendMessageW($List, $LB_GETCOUNT, [IntPtr]::Zero, [IntPtr]::Zero)
    $Items = [System.Collections.Generic.List[string]]::new()
    for ($Index = 0; $Index -lt $Count; $Index++) {
        $Length = [int][ApricotTrendingUiNative]::SendMessageW($List, $LB_GETTEXTLEN, [IntPtr]$Index, [IntPtr]::Zero)
        if ($Length -lt 0) { continue }
        $Buffer = [System.Text.StringBuilder]::new($Length + 1)
        [void][ApricotTrendingUiNative]::SendMessageW($List, $LB_GETTEXT, [IntPtr]$Index, $Buffer)
        $Items.Add($Buffer.ToString())
    }
    return $Items.ToArray()
}

function Wait-Until([scriptblock]$Condition, [int]$TimeoutSeconds, [string]$Failure) {
    $Deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    do {
        $Value = & $Condition
        if ($null -ne $Value -and $Value -ne $false) { return $Value }
        Start-Sleep -Milliseconds 20
    } while ([DateTime]::UtcNow -lt $Deadline)
    throw $Failure
}

function Get-FocusedHandle([IntPtr]$Window) {
    [uint32]$ProcessId = 0
    $ThreadId = [ApricotTrendingUiNative]::GetWindowThreadProcessId($Window, [ref]$ProcessId)
    if ($ThreadId -eq 0) { throw "Could not resolve the application GUI thread" }
    $Info = [ApricotTrendingGuiThreadInfo]::new()
    $Info.Size = [System.Runtime.InteropServices.Marshal]::SizeOf([type][ApricotTrendingGuiThreadInfo])
    if (-not [ApricotTrendingUiNative]::GetGUIThreadInfo($ThreadId, [ref]$Info)) {
        throw "Could not inspect the application focus"
    }
    if ($Info.Focus -eq [IntPtr]::Zero) { throw "The application has no focused control" }
    return $Info.Focus
}

function Send-Tab([IntPtr]$Window) {
    $Handle = Get-FocusedHandle $Window
    [void][ApricotTrendingUiNative]::PostMessageW($Handle, $WM_KEYDOWN, [IntPtr]$VK_TAB, [IntPtr]::Zero)
    [void][ApricotTrendingUiNative]::PostMessageW($Handle, $WM_KEYUP, [IntPtr]$VK_TAB, [IntPtr]::Zero)
    Start-Sleep -Milliseconds 60
}

function Get-MsaaElement([IntPtr]$Window) {
    $InterfaceId = [Guid]"618736e0-3c3d-11cf-810c-00aa00389b71"
    $AccessibleObject = $null
    $Result = [ApricotTrendingUiNative]::AccessibleObjectFromWindow(
        $Window,
        [uint32]4294967292,
        [ref]$InterfaceId,
        [ref]$AccessibleObject
    )
    if ($Result -ne 0 -or $null -eq $AccessibleObject) {
        throw "Could not read the control through MSAA (HRESULT $Result)"
    }
    $Flags = [System.Reflection.BindingFlags]::GetProperty
    $Name = $AccessibleObject.GetType().InvokeMember("accName", $Flags, $null, $AccessibleObject, @([object]0))
    $Role = $AccessibleObject.GetType().InvokeMember("accRole", $Flags, $null, $AccessibleObject, @([object]0))
    return [pscustomobject]@{ Name = $Name; Role = [int]$Role }
}

function Get-ControlClass([IntPtr]$Window) {
    $ClassName = [System.Text.StringBuilder]::new(64)
    [void][ApricotTrendingUiNative]::GetClassNameW($Window, $ClassName, $ClassName.Capacity)
    return $ClassName.ToString()
}

function Find-MessageBox([int]$ProcessId, [IntPtr]$MainWindow) {
    $script:FoundMessageBox = [IntPtr]::Zero
    $Callback = [ApricotTrendingUiNative+EnumWindowsProc]{
        param([IntPtr]$Window, [IntPtr]$Parameter)
        [uint32]$CandidateProcessId = 0
        [void][ApricotTrendingUiNative]::GetWindowThreadProcessId($Window, [ref]$CandidateProcessId)
        if ($CandidateProcessId -ne $Parameter.ToInt32() -or $Window -eq $MainWindow) { return $true }
        $ClassName = [System.Text.StringBuilder]::new(64)
        [void][ApricotTrendingUiNative]::GetClassNameW($Window, $ClassName, $ClassName.Capacity)
        $Title = [System.Text.StringBuilder]::new(128)
        [void][ApricotTrendingUiNative]::GetWindowTextW($Window, $Title, $Title.Capacity)
        if ($ClassName.ToString() -eq "#32770" -and
            $Title.ToString() -eq "ApricotPlayer 2 Beta" -and
            [ApricotTrendingUiNative]::IsWindowVisible($Window)) {
            $script:FoundMessageBox = $Window
            return $false
        }
        return $true
    }
    [void][ApricotTrendingUiNative]::EnumWindows($Callback, [IntPtr]$ProcessId)
    return $script:FoundMessageBox
}

function Start-IsolatedTrendingApp([string]$ApiKey, [string]$Proxy = "") {
    $TemporaryRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("apricot-trending-ui-" + [Guid]::NewGuid().ToString("N"))
    $Roaming = Join-Path $TemporaryRoot "Roaming"
    $SettingsFolder = Join-Path $Roaming "ApricotPlayer2Beta"
    New-Item -ItemType Directory -Path $SettingsFolder -Force | Out-Null
    $SourceSettings = Join-Path $env:APPDATA "ApricotPlayer2Beta\settings.json"
    if (Test-Path -LiteralPath $SourceSettings -PathType Leaf) {
        $Settings = Get-Content -LiteralPath $SourceSettings -Raw | ConvertFrom-Json
    } else {
        throw "A complete beta settings template was not found at $SourceSettings"
    }
    $Settings.enable_trending = $true
    $Settings.youtube_data_api_key = $ApiKey
    $Settings.proxy = $Proxy
    $Settings.language_prompted = $true
    $Settings.main_menu_hidden_actions = @($Settings.main_menu_hidden_actions | Where-Object { $_ -ne "trending" })
    $SettingsJson = $Settings | ConvertTo-Json -Depth 20
    [System.IO.File]::WriteAllText(
        (Join-Path $SettingsFolder "settings.json"),
        $SettingsJson,
        [System.Text.UTF8Encoding]::new($false)
    )

    $OriginalAppData = $env:APPDATA
    try {
        $env:APPDATA = $Roaming
        $Process = Start-Process -FilePath $Executable -WorkingDirectory $InstallRoot -PassThru
    } finally {
        $env:APPDATA = $OriginalAppData
    }
    return [pscustomobject]@{ Process = $Process; TemporaryRoot = $TemporaryRoot }
}

function Open-Trending([System.Diagnostics.Process]$Process) {
    $Window = Wait-Until {
        $Process.Refresh()
        if ($Process.HasExited) { throw "ApricotPlayer 2 Beta exited during startup" }
        if ($Process.MainWindowHandle -ne [IntPtr]::Zero) { return $Process.MainWindowHandle }
        return $null
    } 10 "ApricotPlayer 2 Beta did not create its main window"
    $List = [ApricotTrendingUiNative]::GetDlgItem($Window, $ID_LIST)
    $MenuItems = @(Get-ListStrings $List)
    $TrendingIndex = [Array]::FindIndex($MenuItems, [Predicate[string]]{ param($Item) $Item -match "(?i)trending|v trendu" })
    if ($TrendingIndex -lt 0) { throw "Trending was not present in the main menu" }
    [void][ApricotTrendingUiNative]::SendMessageW($List, $LB_SETCURSEL, [IntPtr]$TrendingIndex, [IntPtr]::Zero)
    [void][ApricotTrendingUiNative]::PostMessageW($Window, $WM_COMMAND, [IntPtr]$ID_OPEN, [IntPtr]::Zero)
    return $Window
}

$Runs = [System.Collections.Generic.List[object]]::new()
$Listener = $null
try {
    $Listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
    $Listener.Start()
    $ProxyPort = ([System.Net.IPEndPoint]$Listener.LocalEndpoint).Port
    $InspectionRun = Start-IsolatedTrendingApp "invalid-qualification-key" "http://127.0.0.1:$ProxyPort"
    $Runs.Add($InspectionRun)
    $Window = Open-Trending $InspectionRun.Process
    $Country = [ApricotTrendingUiNative]::GetDlgItem($Window, $ID_TRENDING_COUNTRY)
    $Category = [ApricotTrendingUiNative]::GetDlgItem($Window, $ID_TRENDING_CATEGORY)
    $Load = [ApricotTrendingUiNative]::GetDlgItem($Window, $ID_LOAD_TRENDING)
    $List = [ApricotTrendingUiNative]::GetDlgItem($Window, $ID_LIST)
    Wait-Until {
        if ([ApricotTrendingUiNative]::IsWindowVisible($Country) -and [ApricotTrendingUiNative]::IsWindowVisible($Category)) { return $true }
        return $false
    } 2 "Trending controls did not become visible" | Out-Null
    Start-Sleep -Milliseconds 500

    $CountryMsaa = Get-MsaaElement $Country
    $CategoryMsaa = Get-MsaaElement $Category
    $LoadMsaa = Get-MsaaElement $Load
    $ListMsaa = Get-MsaaElement $List
    if ((Get-ControlClass $Country) -ne "ComboBox" -or $CountryMsaa.Role -ne 46 -or $CountryMsaa.Name -ne "Trending country") {
        throw "Country filter exposed '$($CountryMsaa.Name)' with MSAA role $($CountryMsaa.Role) and class $(Get-ControlClass $Country)"
    }
    if ((Get-ControlClass $Category) -ne "ComboBox" -or $CategoryMsaa.Role -ne 46 -or $CategoryMsaa.Name -ne "Trending category") {
        throw "Category filter exposed '$($CategoryMsaa.Name)' with MSAA role $($CategoryMsaa.Role) and class $(Get-ControlClass $Category)"
    }
    if ((Get-ControlClass $Load) -ne "Button" -or $LoadMsaa.Role -ne 43 -or $LoadMsaa.Name -ne "Load trending") {
        throw "Load Trending exposed '$($LoadMsaa.Name)' with MSAA role $($LoadMsaa.Role) and class $(Get-ControlClass $Load)"
    }
    if ((Get-ControlClass $List) -ne "ListBox" -or $ListMsaa.Role -ne 33 -or $ListMsaa.Name -ne "Trending") {
        throw "Trending results exposed '$($ListMsaa.Name)' with MSAA role $($ListMsaa.Role) and class $(Get-ControlClass $List)"
    }
    if ([int][ApricotTrendingUiNative]::SendMessageW($Country, $CB_GETCOUNT, [IntPtr]::Zero, [IntPtr]::Zero) -ne 56) {
        throw "Trending country combobox does not contain 56 Python-compatible choices"
    }
    if ([int][ApricotTrendingUiNative]::SendMessageW($Category, $CB_GETCOUNT, [IntPtr]::Zero, [IntPtr]::Zero) -ne 9) {
        throw "Trending category combobox does not contain nine Python-compatible choices"
    }
    if ([int][ApricotTrendingUiNative]::SendMessageW($Country, $CB_GETCURSEL, [IntPtr]::Zero, [IntPtr]::Zero) -ne 0 -or
        [int][ApricotTrendingUiNative]::SendMessageW($Category, $CB_GETCURSEL, [IntPtr]::Zero, [IntPtr]::Zero) -ne 0) {
        throw "Trending filters did not start with the Python-compatible Global and All choices"
    }

    $FocusPath = [System.Collections.Generic.List[string]]::new()
    for ($Index = 0; $Index -lt 5; $Index++) {
        $FocusPath.Add((Get-MsaaElement (Get-FocusedHandle $Window)).Name)
        Send-Tab $Window
    }
    $ExpectedFocusPath = @("Trending country", "Trending category", "Trending", "Back to main menu", "Load trending")
    if (Compare-Object -ReferenceObject $ExpectedFocusPath -DifferenceObject $FocusPath.ToArray() -SyncWindow 0) {
        throw "Unexpected Trending Tab path: $($FocusPath -join ' > ')"
    }
    Write-Output "TRENDING_ACCESSIBILITY_UI=PASS"
    Write-Output "TRENDING_TAB_PATH=$($FocusPath -join ' > ')"

    if (-not $InspectionRun.Process.HasExited) {
        $InspectionRun.Process.Kill()
        $InspectionRun.Process.WaitForExit()
    }

    $ErrorRun = Start-IsolatedTrendingApp ""
    $Runs.Add($ErrorRun)
    $ErrorWindow = Open-Trending $ErrorRun.Process
    $Dialog = Wait-Until {
        $Candidate = Find-MessageBox $ErrorRun.Process.Id $ErrorWindow
        if ($Candidate -ne [IntPtr]::Zero) { return $Candidate }
        return $null
    } 5 "Missing API key did not produce the expected Trending error dialog"
    $DialogFocus = Get-FocusedHandle $ErrorWindow
    [void][ApricotTrendingUiNative]::PostMessageW($DialogFocus, $WM_KEYDOWN, [IntPtr]$VK_RETURN, [IntPtr]::Zero)
    [void][ApricotTrendingUiNative]::PostMessageW($DialogFocus, $WM_KEYUP, [IntPtr]$VK_RETURN, [IntPtr]::Zero)
    Wait-Until {
        $Items = @(Get-ListStrings ([ApricotTrendingUiNative]::GetDlgItem($ErrorWindow, $ID_LIST)))
        if ([Array]::FindIndex($Items, [Predicate[string]]{ param($Item) $Item -match "(?i)search youtube" }) -ge 0) { return $true }
        return $false
    } 5 "Dismissing the Trending error did not return to the main menu" | Out-Null
    Write-Output "TRENDING_NO_KEY_RETURN=PASS"
    Write-Output "YOUTUBE_TRENDING_UI_OK"
}
finally {
    if ($null -ne $Listener) {
        $Listener.Stop()
    }
    foreach ($Run in $Runs) {
        if ($null -ne $Run.Process -and -not $Run.Process.HasExited) {
            $Run.Process.Kill()
            $Run.Process.WaitForExit()
        }
        if (Test-Path -LiteralPath $Run.TemporaryRoot) {
            $ResolvedRoot = [System.IO.Path]::GetFullPath($Run.TemporaryRoot)
            $ExpectedParent = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath()).TrimEnd('\')
            if ((Split-Path -Parent $ResolvedRoot).TrimEnd('\') -ne $ExpectedParent -or
                (Split-Path -Leaf $ResolvedRoot) -notlike "apricot-trending-ui-*") {
                throw "Refusing to remove unexpected qualification path: $ResolvedRoot"
            }
            Remove-Item -LiteralPath $ResolvedRoot -Recurse -Force
        }
    }
}
