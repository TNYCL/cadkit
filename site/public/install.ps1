# cadkit installer for Windows.
#
#   irm https://cadkit.org/install.ps1 | iex
#
# Downloads the prebuilt "cadkit" command-line tool, checks its SHA-256 against the release's
# SHA256SUMS, copies cadkit.exe into a directory of yours and adds that directory to your user
# PATH. It needs no administrator rights. Works in Windows PowerShell 5.1 and PowerShell 7.
#
# Environment:
#   CADKIT_VERSION         version to install, for example 0.5.3 (default: the latest release)
#   CADKIT_INSTALL_DIR     where cadkit.exe goes (default: %LOCALAPPDATA%\Programs\cadkit\bin)
#   CADKIT_DOWNLOAD_URL    release mirror to try first (default: https://release.cadkit.org)
#   CADKIT_NO_MODIFY_PATH  set to 1 to leave the user PATH alone
#
# Files come from release.cadkit.org; when it cannot be reached, the same files are taken
# from the GitHub release. Linux and macOS: https://cadkit.org/install.sh
#
# Source: https://github.com/TNYCL/cadkit/blob/main/site/public/install.ps1
# License: MIT OR Apache-2.0

# One script block: preferences, functions and variables stay out of the caller's session, and
# nothing here calls "exit", which would close the terminal when run through iex.
& {
    $ErrorActionPreference = 'Stop'
    # The progress bar of Invoke-WebRequest slows downloads down in Windows PowerShell 5.1.
    $ProgressPreference = 'SilentlyContinue'

    $repoUrl = 'https://github.com/TNYCL/cadkit'
    $defaultDownloadUrl = 'https://release.cadkit.org'

    function Say([string] $Message) {
        Write-Host "cadkit-install: $Message"
    }

    if ($PSVersionTable.PSVersion.Major -ge 6 -and -not $IsWindows) {
        throw 'cadkit-install: this script is for Windows; use: curl -fsSL https://cadkit.org/install.sh | sh'
    }

    # Windows PowerShell 5.1 on older systems does not offer TLS 1.2 by default.
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

    # PROCESSOR_ARCHITEW6432 is set when a 32-bit PowerShell runs on a 64-bit system.
    $arch = $env:PROCESSOR_ARCHITEW6432
    if (-not $arch) { $arch = $env:PROCESSOR_ARCHITECTURE }
    $target = 'x86_64-pc-windows-msvc'
    if ($arch -eq 'ARM64') {
        Say 'no native ARM64 build yet; installing the x64 build, which Windows 11 runs under emulation'
    } elseif ($arch -ne 'AMD64') {
        throw "cadkit-install: no prebuilt binary for Windows $arch; build from source with: cargo install cadkit-cli"
    }
    $archive = "cadkit-$target.zip"

    $mirror = $env:CADKIT_DOWNLOAD_URL
    if (-not $mirror) { $mirror = $defaultDownloadUrl }
    $mirror = $mirror.TrimEnd('/')

    $installDir = $env:CADKIT_INSTALL_DIR
    if (-not $installDir) {
        if (-not $env:LOCALAPPDATA) { throw 'cadkit-install: LOCALAPPDATA is not set; set CADKIT_INSTALL_DIR' }
        $installDir = Join-Path $env:LOCALAPPDATA 'Programs\cadkit\bin'
    }

    $tmp = Join-Path ([IO.Path]::GetTempPath()) ('cadkit-install-' + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $tmp | Out-Null
    try {
        $zip = Join-Path $tmp $archive
        $sums = Join-Path $tmp 'SHA256SUMS'

        function Get-Download([string] $Url, [string] $File) {
            try {
                Invoke-WebRequest -UseBasicParsing -Uri $Url -OutFile $File
                return $true
            } catch {
                return $false
            }
        }

        # Downloads the archive and SHA256SUMS of one release; $Base is the URL of their directory.
        function Get-Release([string] $Base) {
            Remove-Item -LiteralPath $zip, $sums -Force -ErrorAction SilentlyContinue
            return ((Get-Download "$Base/$archive" $zip) -and (Get-Download "$Base/SHA256SUMS" $sums))
        }

        $version = $env:CADKIT_VERSION
        if ($version) { $version = $version.Trim() -replace '^v', '' }
        if (-not $version) {
            $latest = Join-Path $tmp 'latest.txt'
            if (Get-Download "$mirror/latest.txt" $latest) {
                $version = ([IO.File]::ReadAllText($latest)).Trim()
            }
        }
        # The version becomes part of a URL: accept only what a version can contain.
        if ($version -and $version -notmatch '^[0-9A-Za-z.+-]+$') {
            throw "cadkit-install: not a version: $version"
        }

        $source = $null
        if ($version -and (Get-Release "$mirror/v$version")) {
            $source = $mirror
        } elseif ($version -and (Get-Release "$repoUrl/releases/download/v$version")) {
            $source = 'GitHub Releases'
        } elseif (-not $version -and (Get-Release "$repoUrl/releases/latest/download")) {
            $source = 'GitHub Releases'
        } else {
            throw "cadkit-install: could not download $archive $version from $mirror or $repoUrl/releases"
        }

        $want = $null
        foreach ($line in [IO.File]::ReadAllLines($sums)) {
            $parts = $line.Trim() -split '\s+', 2
            if ($parts.Count -eq 2 -and $parts[1].TrimStart('*') -eq $archive) {
                $want = $parts[0].ToLowerInvariant()
                break
            }
        }
        if (-not $want) { throw "cadkit-install: $archive is not listed in SHA256SUMS" }
        $got = (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($got -ne $want) {
            throw "cadkit-install: checksum mismatch for $archive (expected $want, got $got)"
        }

        Expand-Archive -LiteralPath $zip -DestinationPath $tmp -Force
        $exe = Join-Path $tmp "cadkit-$target\cadkit.exe"
        if (-not (Test-Path -LiteralPath $exe)) {
            throw "cadkit-install: $archive does not contain cadkit-$target\cadkit.exe"
        }

        New-Item -ItemType Directory -Path $installDir -Force | Out-Null
        $dest = Join-Path $installDir 'cadkit.exe'
        Copy-Item -LiteralPath $exe -Destination $dest -Force

        $installed = ("cadkit $version").Trim()
        try { $installed = (& $dest --version | Select-Object -First 1) } catch { }
        Say "installed $installed ($target, from $source) to $dest"

        if ($env:CADKIT_NO_MODIFY_PATH -ne '1') {
            # Read the stored value without expanding %VARIABLES%, so writing it back keeps them.
            $key = Get-Item -LiteralPath 'HKCU:\Environment'
            $stored = [string] $key.GetValue('Path', '', 'DoNotExpandEnvironmentNames')
            $entries = @($stored -split ';' | Where-Object { $_ })
            if ($entries -notcontains $installDir) {
                Set-ItemProperty -LiteralPath 'HKCU:\Environment' -Name 'Path' -Type ExpandString -Value (($entries + $installDir) -join ';')
                # A registry write alone is not announced; setting and removing a variable through
                # .NET broadcasts the change, so new terminals see the new PATH without a sign-out.
                $marker = 'CADKIT_INSTALL_' + [Guid]::NewGuid().ToString('N')
                [Environment]::SetEnvironmentVariable($marker, '1', 'User')
                [Environment]::SetEnvironmentVariable($marker, $null, 'User')
                Say "added $installDir to your user PATH; open a new terminal to use cadkit everywhere"
            }
        }
        if (@($env:Path -split ';') -notcontains $installDir) {
            # This session too, so "cadkit" works right away.
            $env:Path = "$env:Path;$installDir"
            if ($env:CADKIT_NO_MODIFY_PATH -eq '1') {
                Say "$installDir is on PATH for this session only (CADKIT_NO_MODIFY_PATH=1)"
            }
        }
    } finally {
        Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
    }
}
