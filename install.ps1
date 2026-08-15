$ErrorActionPreference = "Stop"

$Repository = if ($env:ASTF_REPOSITORY) { $env:ASTF_REPOSITORY } else { "mfzzf/astraflow-cli" }
$Version = if ($env:ASTF_VERSION) { $env:ASTF_VERSION } else { "latest" }
$Target = switch ($env:PROCESSOR_ARCHITECTURE) {
    "ARM64" { "aarch64-pc-windows-msvc" }
    "AMD64" { "x86_64-pc-windows-msvc" }
    default { throw "astf: unsupported Windows architecture: $env:PROCESSOR_ARCHITECTURE" }
}
$Asset = "astf-$Target.zip"
if ($env:ASTF_RELEASE_BASE_URL) {
    $BaseUrl = $env:ASTF_RELEASE_BASE_URL.TrimEnd('/')
} elseif ($Version -eq "latest") {
    $BaseUrl = "https://github.com/$Repository/releases/latest/download"
} else {
    $Tag = if ($Version.StartsWith("v")) { $Version } else { "v$Version" }
    $BaseUrl = "https://github.com/$Repository/releases/download/$Tag"
}

$TempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("astf-" + [System.Guid]::NewGuid())
New-Item -ItemType Directory -Path $TempDir | Out-Null
try {
    $Archive = Join-Path $TempDir $Asset
    $Sums = Join-Path $TempDir "SHA256SUMS"
    Write-Host "Downloading astf for $Target..."
    Invoke-WebRequest -UseBasicParsing -Uri "$BaseUrl/$Asset" -OutFile $Archive
    Invoke-WebRequest -UseBasicParsing -Uri "$BaseUrl/SHA256SUMS" -OutFile $Sums

    $Entry = Get-Content $Sums | Where-Object { $_ -match "^[0-9a-fA-F]{64}\s+\*?$([regex]::Escape($Asset))$" } | Select-Object -First 1
    if (-not $Entry) { throw "astf: $Asset is missing from SHA256SUMS" }
    $Expected = ($Entry -split '\s+')[0].ToLowerInvariant()
    $Actual = (Get-FileHash -Algorithm SHA256 $Archive).Hash.ToLowerInvariant()
    if ($Actual -ne $Expected) { throw "astf: checksum verification failed" }

    Expand-Archive -Path $Archive -DestinationPath $TempDir -Force
    $InstallDir = if ($env:ASTF_INSTALL_DIR) {
        $env:ASTF_INSTALL_DIR
    } else {
        Join-Path $env:LOCALAPPDATA "Programs\astf\bin"
    }
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    Copy-Item (Join-Path $TempDir "astf.exe") (Join-Path $InstallDir "astf.exe") -Force

    $UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $PathParts = @($UserPath -split ';' | Where-Object { $_ })
    if ($InstallDir -notin $PathParts) {
        $NewPath = (@($PathParts) + $InstallDir) -join ';'
        [Environment]::SetEnvironmentVariable("Path", $NewPath, "User")
        $env:Path = "$InstallDir;$env:Path"
        Write-Host "Added $InstallDir to your user PATH. Open a new terminal to use it everywhere."
    }
    Write-Host "Installed astf to $InstallDir\astf.exe"
} finally {
    Remove-Item -Recurse -Force $TempDir -ErrorAction SilentlyContinue
}
