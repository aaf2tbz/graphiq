# GraphIQ Windows installer
#
# Usage:
#   powershell -ExecutionPolicy Bypass -File .\install.ps1
#   powershell -ExecutionPolicy Bypass -File .\install.ps1 -Version 4.5.0
#   powershell -ExecutionPolicy Bypass -File .\install.ps1 -Uninstall
#
# The default install location is %LOCALAPPDATA%\GraphIQ\bin. The installer
# adds that directory to the current user's PATH without requiring elevation.
# -ArchivePath and -NoPathUpdate are intended for CI and offline installs.

[CmdletBinding()]
param(
    [string]$Version = "",
    [string]$InstallDir = "",
    [string]$ArchivePath = "",
    [switch]$Uninstall,
    [switch]$NoPathUpdate,
    [int]$WaitForPid = 0
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
# Windows PowerShell 5.1 may default to older TLS versions; GitHub requires TLS 1.2.
try {
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
} catch {
    # PowerShell 7/.NET Core does not need this compatibility setting.
}

$Repository = "aaf2tbz/GraphIQ"
$Target = "x86_64-pc-windows-msvc"
$BinaryNames = @("graphiq.exe", "graphiq-mcp.exe", "graphiq-bench.exe")
$ApiHeaders = @{ "User-Agent" = "GraphIQ installer" }

if ([string]::IsNullOrWhiteSpace($InstallDir)) {
    $localAppData = [Environment]::GetFolderPath([Environment+SpecialFolder]::LocalApplicationData)
    if ([string]::IsNullOrWhiteSpace($localAppData)) {
        $localAppData = $env:LOCALAPPDATA
    }
    if ([string]::IsNullOrWhiteSpace($localAppData)) {
        throw "Could not determine the Windows local application-data directory. Pass -InstallDir explicitly."
    }
    $InstallDir = Join-Path $localAppData "GraphIQ\bin"
}
$InstallDir = [IO.Path]::GetFullPath($InstallDir)

function Write-Info([string]$Message) {
    Write-Host "  [GraphIQ] $Message"
}

function Find-ReleaseAsset($Release, [string]$Name) {
    @($Release.assets | Where-Object { $_.name -eq $Name }) | Select-Object -First 1
}

function Get-Release([string]$ReleaseVersion) {
    if ([string]::IsNullOrWhiteSpace($ReleaseVersion)) {
        return Invoke-RestMethod -Uri "https://api.github.com/repos/$Repository/releases/latest" -Headers $ApiHeaders
    }

    $normalized = $ReleaseVersion.Trim()
    if ($normalized.StartsWith("v")) {
        $normalized = $normalized.Substring(1)
    }
    return Invoke-RestMethod -Uri "https://api.github.com/repos/$Repository/releases/tags/v$normalized" -Headers $ApiHeaders
}

function Normalize-PathForCompare([string]$Path) {
    return ($Path.TrimEnd('\')).TrimEnd('/')
}

function Add-InstallDirToUserPath {
    if ($NoPathUpdate) {
        return
    }

    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $parts = @()
    if (-not [string]::IsNullOrWhiteSpace($userPath)) {
        $parts = @($userPath -split ';' | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
    }

    $normalizedInstallDir = Normalize-PathForCompare $InstallDir
    $alreadyPresent = @($parts | Where-Object {
        (Normalize-PathForCompare $_) -ieq $normalizedInstallDir
    }).Count -gt 0

    if (-not $alreadyPresent) {
        $parts += $InstallDir
        [Environment]::SetEnvironmentVariable("Path", ($parts -join ';'), "User")
        $env:Path = "$InstallDir;$env:Path"
        Write-Info "added $InstallDir to the current user's PATH"
    }
}

function Remove-InstallDirFromUserPath {
    if ($NoPathUpdate) {
        return
    }

    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if ([string]::IsNullOrWhiteSpace($userPath)) {
        return
    }

    $normalizedInstallDir = Normalize-PathForCompare $InstallDir
    $parts = @($userPath -split ';' | Where-Object {
        -not [string]::IsNullOrWhiteSpace($_) -and
        (Normalize-PathForCompare $_) -ine $normalizedInstallDir
    })
    [Environment]::SetEnvironmentVariable("Path", ($parts -join ';'), "User")
}

if ($Uninstall) {
    foreach ($binary in $BinaryNames) {
        $destination = Join-Path $InstallDir $binary
        if (Test-Path -LiteralPath $destination) {
            Remove-Item -LiteralPath $destination -Force
            Write-Info "removed $destination"
        }
    }
    Remove-InstallDirFromUserPath
    Write-Info "uninstall complete"
    exit 0
}

$tempRoot = Join-Path ([IO.Path]::GetTempPath()) ("graphiq-install-" + [Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $tempRoot -Force | Out-Null

try {
    $release = $null
    if ([string]::IsNullOrWhiteSpace($ArchivePath)) {
        $release = Get-Release $Version
        $tagName = [string]$release.tag_name
        if ([string]::IsNullOrWhiteSpace($tagName)) {
            throw "GitHub release did not contain a tag name."
        }
        $Version = $tagName.TrimStart('v')

        $archiveName = "graphiq-$Target.zip"
        $archiveAsset = Find-ReleaseAsset $release $archiveName
        if ($null -eq $archiveAsset) {
            throw "Release v$Version does not contain $archiveName."
        }

        $archiveFile = Join-Path $tempRoot $archiveName
        Write-Info "downloading GraphIQ v$Version"
        Invoke-WebRequest -UseBasicParsing -Uri $archiveAsset.browser_download_url -Headers $ApiHeaders -OutFile $archiveFile

        $checksumName = [IO.Path]::ChangeExtension($archiveName, "sha256")
        $checksumAsset = Find-ReleaseAsset $release $checksumName
        if ($null -ne $checksumAsset) {
            Invoke-WebRequest -UseBasicParsing -Uri $checksumAsset.browser_download_url -Headers $ApiHeaders -OutFile (Join-Path $tempRoot $checksumName)
        }
    } else {
        $archiveFile = (Resolve-Path -LiteralPath $ArchivePath).Path
        $archiveName = Split-Path -Leaf $archiveFile
        if ([string]::IsNullOrWhiteSpace($Version)) {
            $Version = "local archive"
        }
        Write-Info "installing $archiveName"
    }

    $checksumFile = [IO.Path]::ChangeExtension($archiveFile, "sha256")
    $expectedHash = $null
    if (Test-Path -LiteralPath $checksumFile) {
        $checksumText = (Get-Content -LiteralPath $checksumFile -Raw).TrimStart([char]0xFEFF)
        $pattern = '(?im)^\s*([0-9a-fA-F]{64})\s+\*?' + [Regex]::Escape($archiveName) + '\s*$'
        $match = [Regex]::Match($checksumText, $pattern)
        if ($match.Success) {
            $expectedHash = $match.Groups[1].Value
        }
    }
    if ([string]::IsNullOrWhiteSpace($expectedHash) -and $null -ne $release) {
        $archiveAsset = Find-ReleaseAsset $release $archiveName
        if ($null -ne $archiveAsset -and -not [string]::IsNullOrWhiteSpace([string]$archiveAsset.digest)) {
            $expectedHash = ([string]$archiveAsset.digest) -replace '^sha256:', ''
        }
    }
    if ([string]::IsNullOrWhiteSpace($expectedHash)) {
        throw "No SHA-256 checksum was available for $archiveName; refusing to install it."
    }

    $actualHash = (Get-FileHash -LiteralPath $archiveFile -Algorithm SHA256).Hash
    if ($actualHash -ine $expectedHash.Trim()) {
        throw "SHA-256 checksum mismatch for $archiveName. Expected $expectedHash, got $actualHash."
    }
    Write-Info "checksum verified"

    $extractDir = Join-Path $tempRoot "extracted"
    Expand-Archive -LiteralPath $archiveFile -DestinationPath $extractDir -Force
    foreach ($binary in $BinaryNames) {
        $source = Join-Path $extractDir $binary
        if (-not (Test-Path -LiteralPath $source -PathType Leaf)) {
            throw "Archive is missing required binary $binary."
        }
    }

    if ($WaitForPid -gt 0) {
        $parent = Get-Process -Id $WaitForPid -ErrorAction SilentlyContinue
        if ($null -ne $parent) {
            Write-Info "waiting for process $WaitForPid to exit before replacing locked binaries"
            Wait-Process -Id $WaitForPid
        }
        Start-Sleep -Milliseconds 500
    }

    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    foreach ($binary in $BinaryNames) {
        $source = Join-Path $extractDir $binary
        $destination = Join-Path $InstallDir $binary
        $temporaryDestination = "$destination.tmp.$PID"
        if (Test-Path -LiteralPath $temporaryDestination) {
            Remove-Item -LiteralPath $temporaryDestination -Force
        }
        Copy-Item -LiteralPath $source -Destination $temporaryDestination -Force
        Move-Item -LiteralPath $temporaryDestination -Destination $destination -Force
        Write-Info "$binary -> $destination"
    }

    Add-InstallDirToUserPath
    Write-Info "installed GraphIQ v$Version"
    Write-Host "  Open a new terminal for PATH changes to take effect."
} finally {
    if (Test-Path -LiteralPath $tempRoot) {
        Remove-Item -LiteralPath $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
}
