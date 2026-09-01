$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type @"
using System;
using System.Runtime.InteropServices;

public struct ApricotRect
{
    public int Left;
    public int Top;
    public int Right;
    public int Bottom;
}

[StructLayout(LayoutKind.Sequential)]
public struct ApricotGuiThreadInfo
{
    public int Size;
    public int Flags;
    public IntPtr Active;
    public IntPtr Focus;
    public IntPtr Capture;
    public IntPtr MenuOwner;
    public IntPtr MoveSize;
    public IntPtr Caret;
    public ApricotRect CaretRect;
}

public static class ApricotNativeMethods
{
    [DllImport("user32.dll")]
    public static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);

    [DllImport("user32.dll")]
    public static extern bool GetGUIThreadInfo(uint threadId, ref ApricotGuiThreadInfo info);

    [DllImport("user32.dll")]
    public static extern bool PostMessage(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);
}
"@

$workspace = Split-Path -Parent $PSScriptRoot
$executable = Join-Path $workspace "target\debug\apricot-windows-accessibility-spike.exe"
if (-not (Test-Path -LiteralPath $executable -PathType Leaf)) {
    throw "Build the accessibility spike before running this script: cargo build -p apricot-windows-accessibility-spike"
}

function Find-Descendant {
    param(
        [Parameter(Mandatory)]
        [System.Windows.Automation.AutomationElement] $Root,
        [Parameter(Mandatory)]
        [string] $Name,
        [System.Windows.Automation.ControlType] $ControlType
    )

    $nameCondition = [System.Windows.Automation.PropertyCondition]::new(
        [System.Windows.Automation.AutomationElement]::NameProperty,
        $Name
    )
    $condition = $nameCondition
    if ($null -ne $ControlType) {
        $typeCondition = [System.Windows.Automation.PropertyCondition]::new(
            [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
            $ControlType
        )
        $condition = [System.Windows.Automation.AndCondition]::new($nameCondition, $typeCondition)
    }
    $Root.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $condition)
}

$process = Start-Process -FilePath $executable -PassThru
try {
    Start-Sleep -Milliseconds 500
    $desktop = [System.Windows.Automation.AutomationElement]::RootElement
    $windowCondition = [System.Windows.Automation.AndCondition]::new(
        [System.Windows.Automation.PropertyCondition]::new(
            [System.Windows.Automation.AutomationElement]::NameProperty,
            "ApricotPlayer Rust accessibility qualification"
        ),
        [System.Windows.Automation.PropertyCondition]::new(
            [System.Windows.Automation.AutomationElement]::ProcessIdProperty,
            $process.Id
        )
    )
    $root = $desktop.FindFirst([System.Windows.Automation.TreeScope]::Children, $windowCondition)
    if ($null -eq $root) {
        throw "The accessibility spike window was not exposed to UI Automation"
    }

    $expectedRoles = @(
        @("Media results", [System.Windows.Automation.ControlType]::List),
        @("Autoplay next item", [System.Windows.Automation.ControlType]::CheckBox),
        @("Videos", [System.Windows.Automation.ControlType]::ComboBox),
        @("Volume, 50 percent", [System.Windows.Automation.ControlType]::Slider),
        @("Open modal dialog", [System.Windows.Automation.ControlType]::Button),
        @("Announce playback status", [System.Windows.Automation.ControlType]::Button),
        @("Download progress, 35 percent", [System.Windows.Automation.ControlType]::ProgressBar),
        @("Embedded video output host", [System.Windows.Automation.ControlType]::Text)
    )
    foreach ($expected in $expectedRoles) {
        $element = Find-Descendant -Root $root -Name $expected[0] -ControlType $expected[1]
        if ($null -eq $element) {
            throw "Missing UIA role $($expected[1].ProgrammaticName) for '$($expected[0])'"
        }
    }
    Write-Output "ROLE_GATE=PASS ($($expectedRoles.Count) controls)"

    $checkbox = Find-Descendant -Root $root -Name "Autoplay next item" -ControlType ([System.Windows.Automation.ControlType]::CheckBox)
    $toggle = $checkbox.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern)
    $before = $toggle.Current.ToggleState
    $toggle.Toggle()
    $after = $toggle.Current.ToggleState
    if ($before -eq $after) {
        throw "Checkbox did not expose a working Toggle pattern"
    }
    Write-Output "CHECKBOX_GATE=PASS ($before->$after)"

    $slider = Find-Descendant -Root $root -Name "Volume, 50 percent" -ControlType ([System.Windows.Automation.ControlType]::Slider)
    $range = $slider.GetCurrentPattern([System.Windows.Automation.RangeValuePattern]::Pattern)
    if ($range.Current.Value -ne 50 -or $range.Current.Minimum -ne 0 -or $range.Current.Maximum -ne 100) {
        throw "Slider did not expose the expected value and range"
    }
    Write-Output "SLIDER_GATE=PASS (50, 0-100)"

    $windowHandle = [IntPtr]::new($root.Current.NativeWindowHandle)
    [uint32] $processId = 0
    $guiThreadId = [ApricotNativeMethods]::GetWindowThreadProcessId($windowHandle, [ref] $processId)
    if ($guiThreadId -eq 0 -or $processId -ne $process.Id) {
        throw "Could not resolve the spike GUI thread"
    }
    $tabPath = @()
    for ($index = 0; $index -lt 7; $index++) {
        $guiInfo = [ApricotGuiThreadInfo]::new()
        $guiInfo.Size = [System.Runtime.InteropServices.Marshal]::SizeOf([type] [ApricotGuiThreadInfo])
        if (-not [ApricotNativeMethods]::GetGUIThreadInfo($guiThreadId, [ref] $guiInfo)) {
            throw "Could not read the spike thread focus"
        }
        if ($guiInfo.Focus -eq [IntPtr]::Zero) {
            throw "The spike thread has no focused control"
        }
        $focused = [System.Windows.Automation.AutomationElement]::FromHandle($guiInfo.Focus)
        $tabPath += $focused.Current.Name
        $null = [ApricotNativeMethods]::PostMessage($guiInfo.Focus, 0x0100, [IntPtr]::new(9), [IntPtr]::Zero)
        $null = [ApricotNativeMethods]::PostMessage($guiInfo.Focus, 0x0101, [IntPtr]::new(9), [IntPtr]::Zero)
        Start-Sleep -Milliseconds 100
    }
    $expectedTabPath = @(
        "Autoplay next item",
        "Videos",
        "Volume, 50 percent",
        "Read-only lyrics and transcript fields use this native control.`r`nTab continues to the next control.",
        "Open modal dialog",
        "Announce playback status",
        "Media results"
    )
    if (Compare-Object -ReferenceObject $expectedTabPath -DifferenceObject $tabPath -SyncWindow 0) {
        throw "Unexpected Tab path: $($tabPath -join ' > ')"
    }
    Write-Output "TAB_GATE=PASS"

    $announce = Find-Descendant -Root $root -Name "Announce playback status" -ControlType ([System.Windows.Automation.ControlType]::Button)
    $invoke = $announce.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)
    $invoke.Invoke()
    Start-Sleep -Milliseconds 100
    $status = Find-Descendant -Root $root -Name "Playback paused at 1 minute 23 seconds" -ControlType ([System.Windows.Automation.ControlType]::Text)
    if ($null -eq $status) {
        throw "Status announcement did not update the accessible name"
    }
    Write-Output "ANNOUNCEMENT_GATE=PASS"

    Write-Output "ACCESSIBILITY_SPIKE=PASS"
}
finally {
    if (-not $process.HasExited) {
        $null = $process.CloseMainWindow()
        Start-Sleep -Milliseconds 200
    }
    if (-not $process.HasExited) {
        Stop-Process -Id $process.Id -Force
    }
}
