[CmdletBinding()]
param(
    [string]$Version = $(if ($env:TRISEEK_VERSION) { $env:TRISEEK_VERSION } else { "latest" }),
    [string]$InstallDir = $(if ($env:TRISEEK_INSTALL_DIR) { $env:TRISEEK_INSTALL_DIR } else { Join-Path $HOME "AppData\Local\Programs\TriSeek\bin" }),
    [string]$Repo = $(if ($env:TRISEEK_REPO) { $env:TRISEEK_REPO } else { "Sagart-cactus/TriSeek" }),
    [switch]$SkipPathUpdate
)

$ErrorActionPreference = "Stop"

if (-not $SkipPathUpdate -and $env:TRISEEK_SKIP_PATH_UPDATE) {
    switch ($env:TRISEEK_SKIP_PATH_UPDATE.ToLowerInvariant()) {
        "1" { $SkipPathUpdate = $true }
        "true" { $SkipPathUpdate = $true }
        "yes" { $SkipPathUpdate = $true }
    }
}

function Normalize-Version {
    param([string]$Value)

    if ([string]::IsNullOrWhiteSpace($Value) -or $Value -eq "latest") {
        return "latest"
    }

    if ($Value.StartsWith("v")) {
        return $Value
    }

    return "v$Value"
}

function Get-TriSeekArch {
    if (-not [Environment]::Is64BitOperatingSystem) {
        throw "TriSeek releases currently require 64-bit Windows."
    }

    if ($env:PROCESSOR_ARCHITECTURE -eq "ARM64") {
        throw "Prebuilt Windows ARM64 releases are not published yet."
    }

    return "x86_64"
}

function Ensure-UserPathEntry {
    param([string]$PathEntry)

    $currentUserPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $entries = @()

    if (-not [string]::IsNullOrWhiteSpace($currentUserPath)) {
        $entries = $currentUserPath.Split(';', [System.StringSplitOptions]::RemoveEmptyEntries)
    }

    if ($entries -contains $PathEntry) {
        return $false
    }

    $newPath = if ([string]::IsNullOrWhiteSpace($currentUserPath)) {
        $PathEntry
    }
    else {
        "$currentUserPath;$PathEntry"
    }

    [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
    return $true
}

function Resolve-TriSeekHome {
    if (-not [string]::IsNullOrWhiteSpace($env:TRISEEK_HOME)) {
        return [System.IO.Path]::GetFullPath($env:TRISEEK_HOME)
    }

    if (-not [string]::IsNullOrWhiteSpace($env:LOCALAPPDATA)) {
        return (Join-Path $env:LOCALAPPDATA "TriSeek")
    }

    return (Join-Path $HOME ".triseek")
}

function Get-DaemonPaths {
    $daemonRoot = Join-Path (Resolve-TriSeekHome) "daemon"
    return @{
        Root = $daemonRoot
        Pid = (Join-Path $daemonRoot "daemon.pid")
        Port = (Join-Path $daemonRoot "daemon.port")
    }
}

function Remove-StaleDaemonFiles {
    $daemonPaths = Get-DaemonPaths
    foreach ($path in @($daemonPaths.Pid, $daemonPaths.Port)) {
        if (Test-Path $path) {
            Remove-Item -Path $path -Force -ErrorAction SilentlyContinue
        }
    }
}

function Get-RunningDaemonPid {
    $daemonPaths = Get-DaemonPaths
    if (-not (Test-Path $daemonPaths.Pid)) {
        return $null
    }

    $raw = (Get-Content -Path $daemonPaths.Pid -Raw -ErrorAction SilentlyContinue).Trim()
    if ([string]::IsNullOrWhiteSpace($raw)) {
        return $null
    }

    [int]$daemonPid = 0
    if (-not [int]::TryParse($raw, [ref]$daemonPid)) {
        return $null
    }

    if (Get-Process -Id $daemonPid -ErrorAction SilentlyContinue) {
        return $daemonPid
    }

    Remove-StaleDaemonFiles
    return $null
}

function Wait-ForDaemonExit {
    param(
        [int]$DaemonPid,
        [int]$TimeoutSeconds = 10
    )

    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        if (-not (Get-Process -Id $DaemonPid -ErrorAction SilentlyContinue)) {
            Remove-StaleDaemonFiles
            return $true
        }
        Start-Sleep -Milliseconds 200
    }

    return $false
}

function Stop-ExistingDaemon {
    param([string]$CliPath)

    $existingDaemonPid = Get-RunningDaemonPid
    if (-not $existingDaemonPid) {
        Remove-StaleDaemonFiles
        return
    }

    if (Test-Path $CliPath) {
        & $CliPath daemon stop *> $null
    }
    elseif (Get-Command triseek -ErrorAction SilentlyContinue) {
        & triseek daemon stop *> $null
    }

    if (Wait-ForDaemonExit -DaemonPid $existingDaemonPid) {
        return
    }

    if (Get-Process -Id $existingDaemonPid -ErrorAction SilentlyContinue) {
        Stop-Process -Id $existingDaemonPid -Force -ErrorAction SilentlyContinue
    }

    if (-not (Wait-ForDaemonExit -DaemonPid $existingDaemonPid)) {
        throw "Failed to stop existing TriSeek daemon (PID $existingDaemonPid)."
    }
}

function Ensure-DaemonRunning {
    param(
        [string]$CliPath,
        [string]$Action
    )

    Write-Host "$Action TriSeek daemon..."
    & $CliPath daemon start
    if ($LASTEXITCODE -ne 0) {
        throw "Failed to start TriSeek daemon after install."
    }
}

function Install-FromCargo {
    param(
        [string]$RepoName,
        [string]$ResolvedVersion,
        [string]$TargetRoot
    )

    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        throw "No matching GitHub Release archive was found and cargo is not installed for source fallback."
    }

    Write-Host "No matching GitHub Release archive was found. Falling back to cargo install."
    New-Item -ItemType Directory -Force -Path $TargetRoot | Out-Null

    $cliArgs = @(
        "install"
        "--locked"
        "--root", $TargetRoot
        "--git", "https://github.com/$RepoName.git"
    )

    if ($ResolvedVersion -ne "latest") {
        $cliArgs += @("--tag", $ResolvedVersion)
    }

    $serverArgs = @($cliArgs)
    $cliArgs += "triseek"
    $serverArgs += "search-server"

    & cargo @cliArgs
    if ($LASTEXITCODE -ne 0) {
        throw "cargo install for triseek failed."
    }

    & cargo @serverArgs
    if ($LASTEXITCODE -ne 0) {
        throw "cargo install for search-server failed."
    }

    return @{
        Cli = (Join-Path $TargetRoot "bin\triseek.exe")
        Server = (Join-Path $TargetRoot "bin\triseek-server.exe")
    }
}

$versionTag = Normalize-Version -Value $Version
$arch = Get-TriSeekArch

if ($versionTag -eq "latest") {
    $archiveName = "triseek-windows-$arch.zip"
    $downloadUrl = "https://github.com/$Repo/releases/latest/download/$archiveName"
}
else {
    $archiveName = "triseek-$versionTag-windows-$arch.zip"
    $downloadUrl = "https://github.com/$Repo/releases/download/$versionTag/$archiveName"
}

$tmpRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("triseek-install-" + [guid]::NewGuid().ToString("N"))
$archivePath = Join-Path $tmpRoot $archiveName
$extractDir = Join-Path $tmpRoot "extract"
$installPath = Join-Path $InstallDir "triseek.exe"
$serverInstallPath = Join-Path $InstallDir "triseek-server.exe"
$hadExistingInstall = (Test-Path $installPath) -or (Test-Path $serverInstallPath)

New-Item -ItemType Directory -Force -Path $tmpRoot | Out-Null
New-Item -ItemType Directory -Force -Path $extractDir | Out-Null
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null

try {
    Write-Host "Downloading $downloadUrl"
    $binaryPaths = $null

    try {
        Invoke-WebRequest -Uri $downloadUrl -OutFile $archivePath
        Expand-Archive -Path $archivePath -DestinationPath $extractDir -Force

        $binary = Get-ChildItem -Path $extractDir -Filter "triseek.exe" -File -Recurse | Select-Object -First 1
        $serverBinary = Get-ChildItem -Path $extractDir -Filter "triseek-server.exe" -File -Recurse | Select-Object -First 1
        if (-not $binary) {
            throw "Downloaded archive did not contain triseek.exe."
        }
        if (-not $serverBinary) {
            throw "Downloaded archive did not contain triseek-server.exe."
        }

        $binaryPaths = @{
            Cli = $binary.FullName
            Server = $serverBinary.FullName
        }
    }
    catch {
        $cargoRoot = Join-Path $tmpRoot "cargo-root"
        $binaryPaths = Install-FromCargo -RepoName $Repo -ResolvedVersion $versionTag -TargetRoot $cargoRoot
    }

    $existingDaemonPid = Get-RunningDaemonPid
    if ($existingDaemonPid) {
        Write-Host "Stopping existing TriSeek daemon (PID $existingDaemonPid)..."
        Stop-ExistingDaemon -CliPath $installPath
    }

    Copy-Item -Path $binaryPaths.Cli -Destination $installPath -Force
    Copy-Item -Path $binaryPaths.Server -Destination $serverInstallPath -Force

    & $installPath help *> $null
    if ($LASTEXITCODE -ne 0) {
        throw "Installed binary did not pass the smoke check."
    }
    & $serverInstallPath --help *> $null
    if ($LASTEXITCODE -ne 0) {
        throw "Installed daemon binary did not pass the smoke check."
    }

    if ($hadExistingInstall -or $existingDaemonPid) {
        Ensure-DaemonRunning -CliPath $installPath -Action "Restarting"
    }
    else {
        Ensure-DaemonRunning -CliPath $installPath -Action "Starting"
    }

    $pathUpdated = $false
    if (-not $SkipPathUpdate) {
        $pathUpdated = Ensure-UserPathEntry -PathEntry $InstallDir
    }

    Write-Host "Installed triseek to $installPath"
    Write-Host "Installed triseek-server to $serverInstallPath"
    if ($pathUpdated) {
        Write-Host "Added $InstallDir to the user PATH. Open a new terminal before using triseek."
    }
    elseif (-not (($env:PATH -split ';') -contains $InstallDir)) {
        Write-Host "Add this directory to PATH if your current terminal cannot find triseek:"
        Write-Host "  $InstallDir"
    }

    Write-Host "Try: triseek help"
}
finally {
    if (Test-Path $tmpRoot) {
        Remove-Item -Path $tmpRoot -Recurse -Force
    }
}
