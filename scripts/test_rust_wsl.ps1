<# Build and test the independent Rust Linux client inside Fedora WSL. #>
[CmdletBinding()]
param(
    [ValidatePattern('^[A-Za-z0-9][A-Za-z0-9._-]*$')]
    [string]$Distro = 'FedoraLinux-44',
    [switch]$Prepare
)
$ErrorActionPreference = 'Stop'
$wslCommand = Get-Command wsl.exe -CommandType Application | Select-Object -First 1
if ($Prepare) {
    & $wslCommand.Source --distribution $Distro --user root --exec dnf install -y cargo rust gcc openssl-devel gtk4-devel clippy rustfmt
    if ($LASTEXITCODE -ne 0) { throw 'Could not prepare Rust development packages in Fedora WSL' }
}
$script = Join-Path $PSScriptRoot 'test_rust_wsl.sh'
$linuxScript = (& $wslCommand.Source --distribution $Distro --exec wslpath -a -u $script | Out-String).Trim()
if ($LASTEXITCODE -ne 0 -or -not $linuxScript) { throw 'Fedora WSL is unavailable or could not map the script path' }
& $wslCommand.Source --distribution $Distro --exec sh $linuxScript
if ($LASTEXITCODE -ne 0) { throw 'Rust validation failed; see output above' }
