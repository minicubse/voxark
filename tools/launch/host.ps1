param([ValidateRange(1024,65535)][int]$Port = 47620, [string]$Config = 'server.json')
. (Join-Path $PSScriptRoot 'common.ps1')
if (!(Test-Path -LiteralPath $ServerExe)) { throw "Serveur absent : $ServerExe" }
if (!(Test-Path -LiteralPath $ClientExe)) { throw "Client absent : $ClientExe" }
$info = New-Object System.Diagnostics.ProcessStartInfo
$info.FileName = $ServerExe
$info.WorkingDirectory = $PackageRoot
$info.UseShellExecute = $false
$info.CreateNoWindow = $true
$info.RedirectStandardInput = $true
$info.Arguments = '--config "{0}" --bind 0.0.0.0:{1}' -f $Config, $Port
$server = New-Object System.Diagnostics.Process
$server.StartInfo = $info
$started = $false
try {
    $started = $server.Start()
    if (!$started) { throw 'Démarrage serveur impossible.' }
    Start-Sleep -Milliseconds 500
    if ($server.HasExited) { throw "Le serveur a quitté avec le code $($server.ExitCode)." }
    Start-PrimalClient -ClientArguments @('--connect', "127.0.0.1:$Port", '--identity', 'profiles/player.token')
}
finally {
    if ($started -and !$server.HasExited) {
        $server.StandardInput.WriteLine('stop')
        $server.StandardInput.Flush()
        if (!$server.WaitForExit(18000)) {
            Write-Warning "Le serveur sauvegarde encore (PID $($server.Id)); il reste ouvert."
        }
    }
    $server.Dispose()
}
