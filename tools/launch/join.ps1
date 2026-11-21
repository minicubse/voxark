param([string]$Address = '127.0.0.1:47620', [string]$Identity = 'profiles/player.token')
. (Join-Path $PSScriptRoot 'common.ps1')
Start-PrimalClient -ClientArguments @('--connect', $Address, '--identity', ('"{0}"' -f $Identity))
