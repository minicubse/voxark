param([string]$Config = 'server.json')
. (Join-Path $PSScriptRoot 'common.ps1')
if (!(Test-Path -LiteralPath $ServerExe)) { throw "Serveur absent : $ServerExe" }
Push-Location $PackageRoot
try { & $ServerExe --config $Config; if ($LASTEXITCODE -ne 0) { throw "Code serveur : $LASTEXITCODE" } }
finally { Pop-Location }
