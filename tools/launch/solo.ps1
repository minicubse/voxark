param([string]$World = 'worlds/solo.pvx')
. (Join-Path $PSScriptRoot 'common.ps1')
Start-PrimalClient -ClientArguments @('--world', ('"{0}"' -f $World))
