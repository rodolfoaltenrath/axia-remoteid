param([string]$Distro = 'FedoraLinux-44')
$ErrorActionPreference = 'Stop'
$wslCommand = Get-Command wsl.exe -CommandType Application | Select-Object -First 1
$script = Join-Path $PSScriptRoot 'test_pkcs11_wsl.sh'
$linuxScript = (& $wslCommand.Source --distribution $Distro --exec wslpath -a -u $script | Out-String).Trim()
if ($LASTEXITCODE -ne 0) { throw 'Fedora WSL unavailable' }
& $wslCommand.Source --distribution $Distro --exec sh $linuxScript
if ($LASTEXITCODE -ne 0) { throw 'PKCS11 laboratory test failed' }
