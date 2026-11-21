$ErrorActionPreference = 'Stop'
$PackageRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$ClientExe = Join-Path $PackageRoot 'PrimalVoxel.exe'
$ServerExe = Join-Path $PackageRoot 'primal-server.exe'
function Start-PrimalClient([string[]]$ClientArguments) {
    if (!(Test-Path -LiteralPath $ClientExe)) { throw "Client absent : $ClientExe" }
    $start = @{ FilePath = $ClientExe; WorkingDirectory = $PackageRoot; PassThru = $true; Wait = $true }
    if ($ClientArguments.Count) { $start.ArgumentList = $ClientArguments }
    $client = Start-Process @start
    if ($client.ExitCode -ne 0) { throw "Le client a quitté avec le code $($client.ExitCode)." }
}
