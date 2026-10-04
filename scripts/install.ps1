# Installs vernier from a GitHub release on Windows.
#
#   irm https://github.com/Go-Vernier/Vernier-OSS/releases/latest/download/install.ps1 | iex
#
# $env:VERNIER_VERSION      a release such as v0.1.0 (default: the latest)
# $env:VERNIER_INSTALL_DIR  where to put vernier.exe
#                           (default: %LOCALAPPDATA%\Programs\vernier\bin)

# A script block, so `irm | iex` leaves nothing behind in the caller's session.
& {
    $ErrorActionPreference = 'Stop'
    $ProgressPreference = 'SilentlyContinue'
    # Windows PowerShell 5.1 on older Windows does not offer TLS 1.2 by default.
    [Net.ServicePointManager]::SecurityProtocol =
        [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

    $repo = 'https://github.com/Go-Vernier/Vernier-OSS'
    $asset = 'vernier-x86_64-pc-windows-msvc.zip'

    # x64 binary; Windows on ARM runs it under emulation.
    $cpu = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
    if ($cpu -ne 'AMD64' -and $cpu -ne 'ARM64') {
        throw "vernier install: there is no prebuilt binary for $cpu Windows; build from source: $repo"
    }

    $base = if ($env:VERNIER_BASE_URL) {
        $env:VERNIER_BASE_URL
    } elseif ($env:VERNIER_VERSION) {
        "$repo/releases/download/v$($env:VERNIER_VERSION.TrimStart('v'))"
    } else {
        "$repo/releases/latest/download"
    }
    $dir = if ($env:VERNIER_INSTALL_DIR) {
        $env:VERNIER_INSTALL_DIR
    } else {
        Join-Path $env:LOCALAPPDATA 'Programs\vernier\bin'
    }

    $tmp = Join-Path ([IO.Path]::GetTempPath()) ("vernier-" + [Guid]::NewGuid())
    New-Item -ItemType Directory -Path $tmp | Out-Null
    try {
        $zip = Join-Path $tmp $asset
        Write-Host "Downloading $asset"
        try {
            Invoke-WebRequest -UseBasicParsing -Uri "$base/$asset" -OutFile $zip
            Invoke-WebRequest -UseBasicParsing -Uri "$base/$asset.sha256" -OutFile "$zip.sha256"
        } catch {
            throw "vernier install: could not download $base/${asset}: $($_.Exception.Message)"
        }
        $expected = ((Get-Content "$zip.sha256" -Raw).Trim() -split '\s+')[0].ToLowerInvariant()
        $actual = (Get-FileHash -Algorithm SHA256 -Path $zip).Hash.ToLowerInvariant()
        if ($expected -ne $actual) {
            throw "vernier install: checksum mismatch for $asset; nothing was installed"
        }
        Expand-Archive -Path $zip -DestinationPath (Join-Path $tmp 'x') -Force
        New-Item -ItemType Directory -Force -Path $dir | Out-Null
        Copy-Item -Force (Join-Path $tmp 'x\vernier.exe') (Join-Path $dir 'vernier.exe')
    } finally {
        Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
    }

    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    $entries = @(if ($userPath) { $userPath -split ';' | Where-Object { $_ } })
    if ($entries -notcontains $dir) {
        [Environment]::SetEnvironmentVariable('Path', (@($entries) + $dir) -join ';', 'User')
        Write-Host "Added $dir to your user PATH. New terminals will find vernier."
    }
    if (($env:Path -split ';') -notcontains $dir) {
        $env:Path = "$env:Path;$dir"
    }

    $version = & (Join-Path $dir 'vernier.exe') --version
    Write-Host "Installed $version to $(Join-Path $dir 'vernier.exe')"
}
