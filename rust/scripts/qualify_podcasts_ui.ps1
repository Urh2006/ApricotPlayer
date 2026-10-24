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
    throw "Close ApricotPlayer 2 Beta before running the podcasts UI qualification"
}

Add-Type -TypeDefinition @"
using System;
using System.Text;
using System.Runtime.InteropServices;

[StructLayout(LayoutKind.Sequential)]
public struct ApricotPodcastsRect
{
    public int Left;
    public int Top;
    public int Right;
    public int Bottom;
}

[StructLayout(LayoutKind.Sequential)]
public struct ApricotPodcastsGuiThreadInfo
{
    public int Size;
    public int Flags;
    public IntPtr Active;
    public IntPtr Focus;
    public IntPtr Capture;
    public IntPtr MenuOwner;
    public IntPtr MoveSize;
    public IntPtr Caret;
    public ApricotPodcastsRect CaretRect;
}

public static class ApricotPodcastsUiNative
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
    public static extern bool GetGUIThreadInfo(uint threadId, ref ApricotPodcastsGuiThreadInfo info);

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
$VK_RETURN = 13
$VK_ESCAPE = 27
$VK_TAB = 9
$ID_LIST = 1001
$ID_OPEN = 1002
$ID_BACK = 1006
$ID_REMOVE = 1016
$ID_RSS_SEARCH = 1030
$ID_RSS_CATEGORIES = 1031
$ID_RSS_ADD = 1032
$ID_RSS_REFRESH = 1033
$ID_RSS_FILTER = 1034
$ID_RSS_SET_CATEGORY = 1035
$ID_RSS_IMPORT = 1036
$ID_RSS_EXPORT = 1037
$ID_RSS_DOWNLOAD_FEED = 1038
$ID_RSS_DOWNLOAD_EPISODE = 1044

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
    $Count = [int][ApricotPodcastsUiNative]::SendMessageW($List, $LB_GETCOUNT, [IntPtr]::Zero, [IntPtr]::Zero)
    $Items = [System.Collections.Generic.List[string]]::new()
    for ($Index = 0; $Index -lt $Count; $Index++) {
        $Length = [int][ApricotPodcastsUiNative]::SendMessageW($List, $LB_GETTEXTLEN, [IntPtr]$Index, [IntPtr]::Zero)
        if ($Length -lt 0) { continue }
        $Buffer = [System.Text.StringBuilder]::new($Length + 1)
        [void][ApricotPodcastsUiNative]::SendMessageW($List, $LB_GETTEXT, [IntPtr]$Index, $Buffer)
        $Items.Add($Buffer.ToString())
    }
    return $Items.ToArray()
}

function Get-FocusedHandle([IntPtr]$Window) {
    [uint32]$ProcessId = 0
    $ThreadId = [ApricotPodcastsUiNative]::GetWindowThreadProcessId($Window, [ref]$ProcessId)
    $Info = [ApricotPodcastsGuiThreadInfo]::new()
    $Info.Size = [System.Runtime.InteropServices.Marshal]::SizeOf([type][ApricotPodcastsGuiThreadInfo])
    if ($ThreadId -eq 0 -or -not [ApricotPodcastsUiNative]::GetGUIThreadInfo($ThreadId, [ref]$Info)) {
        throw "Could not inspect application focus"
    }
    return $Info.Focus
}

function Get-MsaaElement([IntPtr]$Window) {
    $InterfaceId = [Guid]"618736e0-3c3d-11cf-810c-00aa00389b71"
    $AccessibleObject = $null
    $Result = [ApricotPodcastsUiNative]::AccessibleObjectFromWindow(
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
    [void][ApricotPodcastsUiNative]::GetClassNameW($Window, $ClassName, $ClassName.Capacity)
    return $ClassName.ToString()
}

function Send-Key([IntPtr]$Window, [int]$Key) {
    [void][ApricotPodcastsUiNative]::PostMessageW($Window, $WM_KEYDOWN, [IntPtr]$Key, [IntPtr]::Zero)
    [void][ApricotPodcastsUiNative]::PostMessageW($Window, $WM_KEYUP, [IntPtr]$Key, [IntPtr]::Zero)
    Start-Sleep -Milliseconds 50
}

function Assert-Control(
    [IntPtr]$Window,
    [int]$Id,
    [string]$Name,
    [string]$Class,
    [int]$Role
) {
    $Control = [ApricotPodcastsUiNative]::GetDlgItem($Window, $Id)
    if ($Control -eq [IntPtr]::Zero -or -not [ApricotPodcastsUiNative]::IsWindowVisible($Control)) {
        throw "$Name was not visible"
    }
    $Msaa = Get-MsaaElement $Control
    $ActualClass = Get-ControlClass $Control
    if ($Msaa.Name -ne $Name -or $Msaa.Role -ne $Role -or $ActualClass -ne $Class) {
        throw "$Name exposed '$($Msaa.Name)', role $($Msaa.Role), class $ActualClass"
    }
}

function Assert-TabPath([IntPtr]$Window, [string[]]$ExpectedPath, [string]$ViewName) {
    $FocusPath = [System.Collections.Generic.List[string]]::new()
    foreach ($Expected in $ExpectedPath) {
        $FocusPath.Add((Get-MsaaElement (Get-FocusedHandle $Window)).Name)
        Send-Key (Get-FocusedHandle $Window) $VK_TAB
    }
    if (Compare-Object -ReferenceObject $ExpectedPath -DifferenceObject $FocusPath.ToArray() -SyncWindow 0) {
        throw "Unexpected $ViewName Tab path: $($FocusPath -join ' > ')"
    }
    if ((Get-MsaaElement (Get-FocusedHandle $Window)).Name -ne $ExpectedPath[0]) {
        throw "$ViewName Tab path did not wrap to $($ExpectedPath[0])"
    }
    Write-Output "$($ViewName.ToUpperInvariant())_TAB_PATH=$($FocusPath -join ' > ')"
}

$TemporaryRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("apricot-podcasts-ui-" + [Guid]::NewGuid().ToString("N"))
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
    $Settings.enable_podcasts_rss = $true
    $Settings.rss_refresh_on_startup = $false
    $Settings.rss_auto_refresh_enabled = $false
    $Settings.rss_max_items = 25
    $Settings.main_menu_hidden_actions = @($Settings.main_menu_hidden_actions | Where-Object { $_ -ne "rss_feeds" })
    [System.IO.File]::WriteAllText(
        (Join-Path $Data "settings.json"),
        ($Settings | ConvertTo-Json -Depth 20),
        [System.Text.UTF8Encoding]::new($false)
    )

    $Feeds = @(
        [ordered]@{
            title = "Alpha Podcast"
            url = "https://feeds.example/alpha.xml"
            site_url = "https://podcast.example"
            items_complete = $true
            category = "News"
            speed_preset = 1.25
            last_checked = 0
            created_at = 0
            items = @(
                [ordered]@{
                    title = "Episode One"
                    url = "https://media.example/episode-one.mp3"
                    webpage_url = "https://podcast.example/episode-one"
                    kind = "rss_item"
                    source = "podcast"
                    channel = "Alpha Podcast"
                    timestamp = 1700000000
                    duration = "30:00"
                    duration_seconds = 1800
                    played = $false
                },
                [ordered]@{
                    title = "Episode Two"
                    url = "https://media.example/episode-two.mp3"
                    kind = "rss_item"
                    source = "podcast"
                    channel = "Alpha Podcast"
                    duration = "25:00"
                    duration_seconds = 1500
                    played = $true
                }
            )
        }
    )
    [System.IO.File]::WriteAllText(
        (Join-Path $Data "rss_feeds.json"),
        ("[" + ($Feeds[0] | ConvertTo-Json -Depth 20) + "]"),
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
    $List = [ApricotPodcastsUiNative]::GetDlgItem($Window, $ID_LIST)
    $MenuItems = @(Get-ListStrings $List)
    $Index = [Array]::FindIndex($MenuItems, [Predicate[string]]{ param($Item) $Item -match "^Podcasts and RSS feeds" })
    if ($Index -lt 0) { throw "Podcasts and RSS feeds was not present in the main menu" }
    [void][ApricotPodcastsUiNative]::SendMessageW($List, $LB_SETCURSEL, [IntPtr]$Index, [IntPtr]::Zero)
    Send-Key $List $VK_RETURN
    [void](Wait-Until {
        $Back = [ApricotPodcastsUiNative]::GetDlgItem($Window, $ID_BACK)
        $Back -ne [IntPtr]::Zero -and [ApricotPodcastsUiNative]::IsWindowVisible($Back)
    } 5 "Podcasts and RSS feeds did not open from the main menu")

    $FeedControls = @(
        [pscustomobject]@{ Id = $ID_LIST; Name = "Podcasts and RSS feeds"; Class = "ListBox"; Role = 33 },
        [pscustomobject]@{ Id = $ID_BACK; Name = "Back to main menu"; Class = "Button"; Role = 43 },
        [pscustomobject]@{ Id = $ID_RSS_SEARCH; Name = "Search podcasts"; Class = "Button"; Role = 43 },
        [pscustomobject]@{ Id = $ID_RSS_CATEGORIES; Name = "Browse categories"; Class = "Button"; Role = 43 },
        [pscustomobject]@{ Id = $ID_RSS_ADD; Name = "Add feed"; Class = "Button"; Role = 43 },
        [pscustomobject]@{ Id = $ID_RSS_REFRESH; Name = "Refresh feeds"; Class = "Button"; Role = 43 },
        [pscustomobject]@{ Id = $ID_OPEN; Name = "Open feed"; Class = "Button"; Role = 43 },
        [pscustomobject]@{ Id = $ID_REMOVE; Name = "Remove feed"; Class = "Button"; Role = 43 },
        [pscustomobject]@{ Id = $ID_RSS_FILTER; Name = "Filter by category"; Class = "Button"; Role = 43 },
        [pscustomobject]@{ Id = $ID_RSS_SET_CATEGORY; Name = "Set category"; Class = "Button"; Role = 43 },
        [pscustomobject]@{ Id = $ID_RSS_IMPORT; Name = "Import from OPML"; Class = "Button"; Role = 43 },
        [pscustomobject]@{ Id = $ID_RSS_EXPORT; Name = "Export to OPML"; Class = "Button"; Role = 43 }
    )
    foreach ($Expected in $FeedControls) {
        Assert-Control $Window $Expected.Id $Expected.Name $Expected.Class $Expected.Role
    }
    $Rows = @(Get-ListStrings $List)
    if ($Rows.Count -ne 1 -or $Rows[0] -notmatch "^Alpha Podcast \| Category: News \| 2 items \| 1 played \| speed 1\.25x \| last checked") {
        throw "RSS feed row did not preserve the Python field order: $($Rows -join ' / ')"
    }
    Assert-TabPath $Window @(
        "Podcasts and RSS feeds",
        "Back to main menu",
        "Search podcasts",
        "Browse categories",
        "Add feed",
        "Refresh feeds",
        "Open feed",
        "Remove feed",
        "Filter by category",
        "Set category",
        "Import from OPML",
        "Export to OPML"
    ) "rss_feeds"
    Write-Output "PODCAST_FEEDS_ACCESSIBILITY_UI=PASS"

    Send-Key $List $VK_RETURN
    [void](Wait-Until {
        (Get-MsaaElement $List).Name -eq "Feed items"
    } 5 "Enter on an RSS feed did not open its episodes")
    $EpisodeControls = @(
        [pscustomobject]@{ Id = $ID_LIST; Name = "Feed items"; Class = "ListBox"; Role = 33 },
        [pscustomobject]@{ Id = $ID_BACK; Name = "Back to main menu"; Class = "Button"; Role = 43 },
        [pscustomobject]@{ Id = $ID_RSS_REFRESH; Name = "Refresh feed"; Class = "Button"; Role = 43 },
        [pscustomobject]@{ Id = $ID_OPEN; Name = "Play episode"; Class = "Button"; Role = 43 },
        [pscustomobject]@{ Id = $ID_RSS_DOWNLOAD_EPISODE; Name = "Download episode audio"; Class = "Button"; Role = 43 },
        [pscustomobject]@{ Id = $ID_RSS_DOWNLOAD_FEED; Name = "Download entire feed"; Class = "Button"; Role = 43 }
    )
    foreach ($Expected in $EpisodeControls) {
        Assert-Control $Window $Expected.Id $Expected.Name $Expected.Class $Expected.Role
    }
    $EpisodeRows = @(Get-ListStrings $List)
    if ($EpisodeRows.Count -ne 2 -or
        $EpisodeRows[0] -notmatch "^Episode One \| published: .+ \| 30:00 \| Podcast episode$" -or
        $EpisodeRows[1] -ne "Episode Two | played | 25:00 | Podcast episode") {
        throw "RSS episode rows did not match the accessible Python field order: $($EpisodeRows -join ' / ')"
    }
    Assert-TabPath $Window @(
        "Feed items",
        "Back to main menu",
        "Refresh feed",
        "Play episode",
        "Download episode audio",
        "Download entire feed"
    ) "rss_items"
    Write-Output "PODCAST_EPISODES_ACCESSIBILITY_UI=PASS"

    Send-Key (Get-FocusedHandle $Window) $VK_ESCAPE
    [void](Wait-Until {
        (Get-MsaaElement $List).Name -eq "Podcasts and RSS feeds"
    } 5 "Escape from episodes did not return to RSS feeds")
    [void][ApricotPodcastsUiNative]::PostMessageW($Window, $WM_COMMAND, [IntPtr]$ID_RSS_CATEGORIES, [IntPtr]::Zero)
    [void](Wait-Until {
        (Get-MsaaElement $List).Name -eq "Podcast Categories"
    } 5 "Podcast categories did not open")
    Assert-Control $Window $ID_LIST "Podcast Categories" "ListBox" 33
    Assert-Control $Window $ID_BACK "Back to main menu" "Button" 43
    Assert-Control $Window $ID_OPEN "Open" "Button" 43
    $CategoryRows = @(Get-ListStrings $List)
    if ($CategoryRows.Count -lt 10 -or $CategoryRows[0] -ne "Arts") {
        throw "Podcast category list was incomplete: $($CategoryRows -join ' / ')"
    }
    Assert-TabPath $Window @("Podcast Categories", "Back to main menu", "Open") "podcast_categories"
    Write-Output "PODCAST_CATEGORIES_ACCESSIBILITY_UI=PASS"

    Send-Key (Get-FocusedHandle $Window) $VK_ESCAPE
    [void](Wait-Until {
        (Get-MsaaElement $List).Name -eq "Podcasts and RSS feeds"
    } 5 "Escape from podcast categories did not return to RSS feeds")
    Send-Key (Get-FocusedHandle $Window) $VK_ESCAPE
    $ReturnedItems = @(Get-ListStrings $List)
    if (-not ($ReturnedItems -match "^Podcasts and RSS feeds")) {
        throw "Escape did not return from Podcasts and RSS feeds to the main menu"
    }
    Write-Output "PODCAST_ESCAPE_STACK=PASS"
    Write-Output "PODCASTS_UI_OK"
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
