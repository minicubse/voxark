$ErrorActionPreference = 'Stop'
$PackageRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$BevyExe = Join-Path $PackageRoot 'PrimalVoxel-Bevy.exe'
if (!(Test-Path -LiteralPath $BevyExe)) { throw "Client Bevy absent : $BevyExe" }
$env:WGPU_BACKEND = 'vulkan'
Push-Location -LiteralPath $PackageRoot
try {
    & $BevyExe @args
    $clientExit = $LASTEXITCODE
} finally {
    Pop-Location
}
exit $clientExit
