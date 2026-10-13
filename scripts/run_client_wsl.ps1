<# Forward client arguments to Linux, preserving interactive password input. #>
$ErrorActionPreference = 'Stop'
$wslCommand = Get-Command wsl.exe -CommandType Application | Select-Object -First 1
$script = Join-Path $PSScriptRoot 'run_client_wsl.sh'
$linuxScript = (& $wslCommand.Source --distribution FedoraLinux-44 --exec wslpath -a -u $script | Out-String).Trim()
if ($LASTEXITCODE -ne 0 -or -not $linuxScript) { throw 'Fedora WSL is unavailable' }
& $wslCommand.Source --distribution FedoraLinux-44 --exec sh $linuxScript @args
exit $LASTEXITCODE
