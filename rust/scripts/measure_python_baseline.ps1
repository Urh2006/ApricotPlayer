param(
    [ValidateRange(1, 20)]
    [int] $Iterations = 5
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
Add-Type -AssemblyName UIAutomationClient

$repository = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$python = Join-Path $repository ".venv\Scripts\python.exe"
$entrypoint = Join-Path $repository "wx_main.py"
if (-not (Test-Path -LiteralPath $python -PathType Leaf)) {
    throw "Python baseline interpreter was not found: $python"
}

$existingApricot = Get-CimInstance Win32_Process | Where-Object {
    $_.Name -eq "ApricotPlayer.exe" -or
    ($_.Name -match "^pythonw?\.exe$" -and $_.CommandLine -match "wx_main\.py")
}
if ($existingApricot) {
    $identifiers = ($existingApricot | ForEach-Object { "$($_.Name) PID $($_.ProcessId)" }) -join ", "
    throw "Close the running ApricotPlayer before measuring the Python baseline: $identifiers"
}

$profile = Join-Path $env:TEMP "apricot-rust-python-baseline"
[System.IO.Directory]::CreateDirectory($profile) | Out-Null
$desktop = [System.Windows.Automation.AutomationElement]::RootElement

function Get-OwnedProcessIds {
    param([int] $RootProcessId)

    $snapshot = Get-CimInstance Win32_Process
    $owned = @($RootProcessId)
    $changed = $true
    while ($changed) {
        $changed = $false
        foreach ($candidate in $snapshot) {
            if ($candidate.ParentProcessId -in $owned -and $candidate.ProcessId -notin $owned) {
                $owned += [int] $candidate.ProcessId
                $changed = $true
            }
        }
    }
    $owned
}

function Stop-OwnedProcesses {
    param([int[]] $ProcessIds)

    foreach ($targetId in ($ProcessIds | Sort-Object -Descending -Unique)) {
        Stop-Process -Id $targetId -Force -ErrorAction SilentlyContinue
    }
}

function Measure-OneLaunch {
    $watch = [System.Diagnostics.Stopwatch]::StartNew()
    $launcher = Start-Process `
        -FilePath $python `
        -ArgumentList @($entrypoint) `
        -WorkingDirectory $repository `
        -Environment @{
            APPDATA = $profile
            LOCALAPPDATA = $profile
            PYTHONDONTWRITEBYTECODE = "1"
        } `
        -PassThru
    $owned = @($launcher.Id)
    try {
        $window = $null
        $guiProcessId = 0
        $mainMenuCondition = [System.Windows.Automation.AndCondition]::new(
            [System.Windows.Automation.PropertyCondition]::new(
                [System.Windows.Automation.AutomationElement]::NameProperty,
                "Main menu"
            ),
            [System.Windows.Automation.PropertyCondition]::new(
                [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
                [System.Windows.Automation.ControlType]::List
            )
        )
        for ($attempt = 0; $attempt -lt 400 -and $null -eq $window; $attempt++) {
            $owned = Get-OwnedProcessIds -RootProcessId $launcher.Id
            foreach ($candidateId in $owned) {
                $condition = [System.Windows.Automation.PropertyCondition]::new(
                    [System.Windows.Automation.AutomationElement]::ProcessIdProperty,
                    $candidateId
                )
                $candidateWindows = $desktop.FindAll(
                    [System.Windows.Automation.TreeScope]::Children,
                    $condition
                )
                for ($windowIndex = 0; $windowIndex -lt $candidateWindows.Count; $windowIndex++) {
                    $candidateWindow = $candidateWindows.Item($windowIndex)
                    if (
                        $candidateWindow.Current.ControlType -eq [System.Windows.Automation.ControlType]::Window -and
                        $null -ne $candidateWindow.FindFirst(
                            [System.Windows.Automation.TreeScope]::Descendants,
                            $mainMenuCondition
                        )
                    ) {
                        $window = $candidateWindow
                        $guiProcessId = $candidateId
                        break
                    }
                }
                if ($null -ne $window) {
                    break
                }
            }
            if ($null -eq $window) {
                Start-Sleep -Milliseconds 25
            }
        }
        $watch.Stop()
        if ($null -eq $window) {
            throw "Python accessible Main menu did not appear within 10 seconds"
        }

        $gui = Get-Process -Id $guiProcessId
        $cpuBefore = $gui.TotalProcessorTime
        $idleWatch = [System.Diagnostics.Stopwatch]::StartNew()
        Start-Sleep -Seconds 1
        $idleWatch.Stop()
        $gui.Refresh()
        $cpuAfter = $gui.TotalProcessorTime
        $cpuPercent = 100 * ($cpuAfter - $cpuBefore).TotalSeconds /
            ($idleWatch.Elapsed.TotalSeconds * [Environment]::ProcessorCount)

        [pscustomobject]@{
            LaunchMs = [math]::Round($watch.Elapsed.TotalMilliseconds, 2)
            WorkingSetMB = [math]::Round($gui.WorkingSet64 / 1MB, 2)
            PrivateMB = [math]::Round($gui.PrivateMemorySize64 / 1MB, 2)
            IdleCpuPercent = [math]::Round($cpuPercent, 3)
            Title = $window.Current.Name
        }
    }
    finally {
        $owned = Get-OwnedProcessIds -RootProcessId $launcher.Id
        Stop-OwnedProcesses -ProcessIds $owned
        Start-Sleep -Milliseconds 100
    }
}

$measurements = for ($iteration = 1; $iteration -le $Iterations; $iteration++) {
    $measurement = Measure-OneLaunch
    $measurement | Add-Member -NotePropertyName Iteration -NotePropertyValue $iteration -PassThru
}

$sortedLaunch = @($measurements.LaunchMs | Sort-Object)
$medianIndex = [math]::Floor(($sortedLaunch.Count - 1) / 2)
$p95Index = [math]::Ceiling($sortedLaunch.Count * 0.95) - 1
$summary = [pscustomobject]@{
    Iterations = $Iterations
    LaunchMedianMs = $sortedLaunch[$medianIndex]
    LaunchP95Ms = $sortedLaunch[$p95Index]
    WorkingSetMedianMB = (@($measurements.WorkingSetMB | Sort-Object))[$medianIndex]
    PrivateMedianMB = (@($measurements.PrivateMB | Sort-Object))[$medianIndex]
    IdleCpuMedianPercent = (@($measurements.IdleCpuPercent | Sort-Object))[$medianIndex]
}

[pscustomobject]@{
    Measurements = $measurements
    Summary = $summary
} | ConvertTo-Json -Depth 4
