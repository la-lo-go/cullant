param(
    [string]$Device = '100.89.241.106:45353',
    [string]$Output = '.playwright-mcp/android-video',
    [string]$VideoName = 'DSCF5044.MOV'
)

# Failure modes: a native error glyph replaces the poster; the external warning
# waits for a tap; the user changes the foreground app during the capture.
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$destination = [System.IO.Path]::GetFullPath($Output)
New-Item -ItemType Directory -Path $destination -Force | Out-Null

function Invoke-Adb {
    param([string[]]$Arguments)
    $previousAction = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $reply = & adb -s $Device @Arguments 2>&1
        $exitCode = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $previousAction
    }
    if ($exitCode -ne 0) { throw ($reply -join [Environment]::NewLine) }
    return $reply
}

function Assert-Foreground {
    $focus = Invoke-Adb -Arguments @('shell', 'dumpsys', 'window')
    $line = $focus | Select-String 'mCurrentFocus='
    if ("$line" -notmatch 'org\.cullant\.app') { throw 'Leave Cullant in the foreground during this test.' }
}

function Read-Ui {
    Assert-Foreground
    $null = Invoke-Adb -Arguments @('shell', 'uiautomator', 'dump', '/sdcard/cullant-video-test.xml')
    [xml]$tree = (Invoke-Adb -Arguments @('shell', 'cat', '/sdcard/cullant-video-test.xml')) -join [Environment]::NewLine
    return $tree
}

function Wait-Warning {
    $timer = [System.Diagnostics.Stopwatch]::StartNew()
    do {
        $tree = Read-Ui
        if ($tree.SelectNodes('//node') | Where-Object { $_.text -match "This video can't be played here" }) { return $tree }
    } while ($timer.Elapsed.TotalSeconds -lt 15)
    return $tree
}

function Get-Center {
    param($Node)
    $numbers = [regex]::Matches($Node.bounds, '\d+') | ForEach-Object { [int]$_.Value }
    if ($numbers.Count -ne 4) { throw 'The control has no screen bounds.' }
    return @{ X = [int](($numbers[0] + $numbers[2]) / 2); Y = [int](($numbers[1] + $numbers[3]) / 2) }
}

function Tap-Node {
    param($Node)
    Assert-Foreground
    $point = Get-Center $Node
    $null = Invoke-Adb -Arguments @('shell', 'input', 'tap', "$($point.X)", "$($point.Y)")
}

function Save-Screen {
    param([string]$Name)
    Assert-Foreground
    $null = Invoke-Adb -Arguments @('shell', 'screencap', '-p', '/sdcard/cullant-video-test.png')
    $path = Join-Path $destination "$Name.png"
    $null = Invoke-Adb -Arguments @('pull', '/sdcard/cullant-video-test.png', $path)
    return $path
}

function Poster-Difference {
    param([string]$Before, [string]$Reference)
    $first = [System.Drawing.Bitmap]::new($Before)
    $good = [System.Drawing.Bitmap]::new($Reference)
    try {
        if ($first.Size -ne $good.Size) { throw 'The screen size changed during the test.' }
        $difference = 0
        $samples = 0
        for ($row = 0; $row -lt 15; $row++) {
            for ($column = 0; $column -lt 15; $column++) {
                $x = [int]($first.Width * (0.1 + 0.8 * $column / 14))
                $y = [int]($first.Height * (0.42 + 0.16 * $row / 14))
                $a = $first.GetPixel($x, $y)
                $b = $good.GetPixel($x, $y)
                $difference += [Math]::Abs([int]$a.R - [int]$b.R) + [Math]::Abs([int]$a.G - [int]$b.G) + [Math]::Abs([int]$a.B - [int]$b.B)
                $samples++
            }
        }
        return [Math]::Round($difference / (3 * $samples), 2)
    } finally {
        $first.Dispose()
        $good.Dispose()
    }
}

$results = @()
try {
    Assert-Foreground
    $tree = Read-Ui
    $grid = $tree.SelectSingleNode('//node[@content-desc="Grid"]')
    if (!$grid) { throw 'The project toolbar is not visible.' }
    Tap-Node $grid
    $tree = Read-Ui
    $cell = $tree.SelectNodes('//node') | Where-Object { $_.text -ieq $VideoName -and $_.clickable -eq 'true' } | Select-Object -First 1
    if (!$cell) { throw "The Videos grid must show $VideoName." }
    $point = Get-Center $cell
    Assert-Foreground
    $null = Invoke-Adb -Arguments @('shell', "input tap $($point.X) $($point.Y); sleep 0.5")
    $initial = Save-Screen 'initial'
    $tree = Wait-Warning
    $tree.Save((Join-Path $destination 'before-play.xml'))
    $initialWarning = [bool]($tree.SelectNodes('//node') | Where-Object { $_.text -match "This video can't be played here" })
    $settled = Save-Screen 'settled'
    $screen = [System.Drawing.Bitmap]::new($settled)
    try { $x = [int]($screen.Width / 2); $y = [int]($screen.Height / 2) } finally { $screen.Dispose() }
    Assert-Foreground
    $null = Invoke-Adb -Arguments @('shell', "input tap $x $y; sleep 1")
    $tree = Wait-Warning
    $afterWarning = [bool]($tree.SelectNodes('//node') | Where-Object { $_.text -match "This video can't be played here" })
    $tree.Save((Join-Path $destination 'after-tap.xml'))
    $reference = Save-Screen 'after-tap'
    if (!$afterWarning) { throw 'This clip did not produce an external-player warning. Use an unsupported camera clip.' }
    foreach ($entry in @(@{Name='Initial poster survives without another tap';Path=$initial}, @{Name='Settled poster survives without another tap';Path=$settled})) {
        $delta = Poster-Difference $entry.Path $reference
        $results += @{name=$entry.Name;status=$(if($delta -le 20){'PASS'}else{'FAIL'});meanChannelDifference=$delta;limit=20}
    }
    $results += @{name='External-player warning appears before Play';status=$(if($initialWarning){'PASS'}else{'FAIL'})}
} catch {
    $results += @{name='Device reproduction';status='FAIL';error=$_.Exception.Message}
} finally {
    @{date=[DateTime]::UtcNow.ToString('o');device=$Device;video=$VideoName;results=$results} | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $destination 'results.json') -Encoding utf8
}
$results | ForEach-Object { Write-Output "$($_.status): $($_.name) $($_.error)" }
if ($results.status -contains 'FAIL') { exit 1 }
