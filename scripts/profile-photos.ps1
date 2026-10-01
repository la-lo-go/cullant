param(
    [string]$Root = 'D:\',
    [string]$Output = 'artifacts/performance/2026-10-01-d-drive',
    [string]$Run = 'first-2560',
    [string]$Binary = 'src-tauri/target/release/cullant.exe',
    [ValidateSet(1600,2560,3840)][int]$Edge = 2560,
    [ValidateSet('first','reset','warm')][string]$Cache = 'warm',
    [switch]$Disabled,
    [switch]$QuietNavigation,
    [switch]$MeasureNavigation
)
$ErrorActionPreference = 'Stop'
$outputPath = [IO.Path]::GetFullPath($Output)
$rootPath = [IO.Path]::GetFullPath($Root)
$null = New-Item -ItemType Directory -Force -Path $outputPath
if ($Cache -ne 'warm') {
    & node scripts/profile-photos.mjs "--root=$rootPath" "--output=$outputPath" "--prepare=$Cache"
    if ($LASTEXITCODE -ne 0) { throw 'Project preparation failed' }
}
$profilePath = Join-Path $outputPath "$Run-profile.jsonl"
if (Test-Path -LiteralPath $profilePath) { throw 'Run name already exists' }
if ($Disabled) { Remove-Item Env:CULLANT_PHOTO_PROFILE -ErrorAction SilentlyContinue }
else { $env:CULLANT_PHOTO_PROFILE = $profilePath }
Remove-Item Env:CULLANT_OPEN_PROJECT -ErrorAction SilentlyContinue
$app = Start-Process -FilePath ([IO.Path]::GetFullPath($Binary)) -WindowStyle Hidden -PassThru
$runnerArgs = @('scripts/profile-photos.mjs', "--root=$rootPath", "--output=$outputPath", "--run=$Run", "--edge=$Edge")
if ($QuietNavigation) { $runnerArgs += '--quiet=true' }
if ($MeasureNavigation) { $runnerArgs += '--measureNavigation=true' }
$runner = $null
$samplePath = Join-Path $outputPath "$Run-resources.jsonl"
try {
    # Capture startup and idle samples before the runner opens the project.
    $start = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
    $sampleIndex = 0
    do {
        $now = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
        $processes = @(Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.Id -eq $app.Id -or $_.ProcessName -in @('cullant','msedgewebview2') } | Select-Object Id,ProcessName,CPU,WorkingSet64,PrivateMemorySize64)
        $disk = @(Get-CimInstance Win32_PerfFormattedData_PerfDisk_PhysicalDisk | Where-Object Name -eq '1 D:' | Select-Object Name,DiskReadBytesPersec,DiskWriteBytesPersec,AvgDiskQueueLength,PercentDiskTime)
        $gpu = @(Get-CimInstance Win32_PerfFormattedData_GPUPerformanceCounters_GPUEngine -ErrorAction SilentlyContinue | Where-Object UtilizationPercentage -gt 0 | Select-Object Name,UtilizationPercentage)
        @{ unixMs=$now; appPid=$app.Id; processes=$processes; disk=$disk; gpu=$gpu } | ConvertTo-Json -Depth 5 -Compress | Add-Content -LiteralPath $samplePath -Encoding UTF8
        if (!$runner -and $sampleIndex -ge 4) {
            Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,Name | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $outputPath "$Run-processes.json") -Encoding UTF8
            $runner = Start-Process -FilePath 'node' -ArgumentList $runnerArgs -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $outputPath "$Run-runner.log") -RedirectStandardError (Join-Path $outputPath "$Run-runner-errors.log")
        }
        $sampleIndex++
        Start-Sleep -Milliseconds 1000
        if ($runner) { $runner.Refresh() }
        $app.Refresh()
        if ($app.HasExited) { throw 'Measured application exited early' }
        if ($now - $start -gt 1200000) { throw 'Run exceeded 20 minutes' }
    } while (!$runner -or !$runner.HasExited)
    if (!(Test-Path -LiteralPath (Join-Path $outputPath "$Run-result.json"))) {
        Get-Content -LiteralPath (Join-Path $outputPath "$Run-runner-errors.log")
        throw 'E2E runner did not produce a result'
    }
    Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,Name | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $outputPath "$Run-processes-end.json") -Encoding UTF8
    Get-Content -LiteralPath (Join-Path $outputPath "$Run-runner.log")
} finally {
    if ($runner -and !$runner.HasExited) { Stop-Process -Id $runner.Id }
    if (!$app.HasExited) { Stop-Process -Id $app.Id }
    Remove-Item Env:CULLANT_PHOTO_PROFILE -ErrorAction SilentlyContinue
}
