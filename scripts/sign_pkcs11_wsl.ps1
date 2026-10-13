$ErrorActionPreference = 'Stop'
$wslCommand = Get-Command wsl.exe -CommandType Application | Select-Object -First 1
$script = Join-Path $PSScriptRoot 'sign_pkcs11_wsl.sh'
$linuxScript = (& $wslCommand.Source --distribution FedoraLinux-44 --exec wslpath -a -u $script | Out-String).Trim()
if ($LASTEXITCODE -ne 0) { throw 'Fedora WSL unavailable' }
& $wslCommand.Source --distribution FedoraLinux-44 --exec sh $linuxScript @args
exit $LASTEXITCODE
