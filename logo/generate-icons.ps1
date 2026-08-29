$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$tauriIcons = Join-Path $repoRoot "src-tauri/icons"
$androidRes = Join-Path $repoRoot "src-tauri/gen/android/app/src/main/res"
$tempRoot = Join-Path ([IO.Path]::GetTempPath()) ("cullant-icons-" + [Guid]::NewGuid())
$platformOutput = Join-Path $tempRoot "platforms"
$foregroundOutput = Join-Path $tempRoot "foreground"

function Invoke-TauriIcon {
    param([string[]]$Arguments)

    & npx tauri icon @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "tauri icon failed with exit code $LASTEXITCODE"
    }
}

function Remove-GeneratedDirectory {
    param([string]$Path, [string]$AllowedParent)

    if (-not (Test-Path -LiteralPath $Path)) {
        return
    }

    $resolvedPath = (Resolve-Path -LiteralPath $Path).Path
    $resolvedParent = (Resolve-Path -LiteralPath $AllowedParent).Path.TrimEnd([IO.Path]::DirectorySeparatorChar)
    if (-not $resolvedPath.StartsWith($resolvedParent + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Refusing to remove generated directory outside $resolvedParent"
    }

    Remove-Item -LiteralPath $resolvedPath -Recurse -Force
}

New-Item -ItemType Directory -Path $tempRoot | Out-Null

try {
    Push-Location $repoRoot
    try {
        Invoke-TauriIcon @((Join-Path $PSScriptRoot "icon-square.svg"), "-o", $tauriIcons)
        Invoke-TauriIcon @((Join-Path $PSScriptRoot "icon-manifest.json"), "-o", $platformOutput)

        foreach ($size in 108, 162, 216, 324, 432) {
            Invoke-TauriIcon @((Join-Path $PSScriptRoot "android-foreground.svg"), "-o", $foregroundOutput, "-p", $size)
        }
    }
    finally {
        Pop-Location
    }

    $densities = [ordered]@{
        mdpi = 108
        hdpi = 162
        xhdpi = 216
        xxhdpi = 324
        xxxhdpi = 432
    }

    foreach ($entry in $densities.GetEnumerator()) {
        $mipmap = Join-Path $androidRes ("mipmap-" + $entry.Key)
        $generatedMipmap = Join-Path $platformOutput ("android/mipmap-" + $entry.Key)
        Copy-Item -LiteralPath (Join-Path $generatedMipmap "ic_launcher.png") -Destination $mipmap -Force
        Copy-Item -LiteralPath (Join-Path $generatedMipmap "ic_launcher_round.png") -Destination $mipmap -Force
        Copy-Item -LiteralPath (Join-Path $foregroundOutput ("{0}x{0}.png" -f $entry.Value)) -Destination (Join-Path $mipmap "ic_launcher_foreground.png") -Force
    }

    Copy-Item -LiteralPath (Join-Path $PSScriptRoot "android-adaptive-icon.xml") -Destination (Join-Path $androidRes "mipmap-anydpi-v26/ic_launcher.xml") -Force
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot "android-background-color.xml") -Destination (Join-Path $androidRes "values/ic_launcher_background.xml") -Force

    # This project reads Android assets from gen/android, not this Tauri output.
    Remove-GeneratedDirectory (Join-Path $tauriIcons "android") $tauriIcons
}
finally {
    Remove-GeneratedDirectory $tempRoot ([IO.Path]::GetTempPath())
}
