param(
    [Parameter(Mandatory=$true)][string]$Server,
    [string]$Client = '../../work/target-2/release/primal-client.exe',
    [int]$Repetitions = 3,
    [int]$Frames = 720,
    [string]$Label = 'final',
    [string]$Output = 'benchmarks/results/2/graphics'
)
$ErrorActionPreference = 'Stop'
$Server = (Resolve-Path -LiteralPath $Server).Path
$Client = (Resolve-Path -LiteralPath $Client).Path
$Output = [IO.Path]::GetFullPath($Output)
[void][IO.Directory]::CreateDirectory($Output)
$scratch = [IO.Path]::GetFullPath('../../work/graphics-' + [Guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($scratch)

function Start-Primal([string]$Executable, [string[]]$Arguments) {
    $info = New-Object Diagnostics.ProcessStartInfo
    $info.FileName = $Executable
    $info.Arguments = ($Arguments | ForEach-Object { '"' + $_.Replace('"', '\"') + '"' }) -join ' '
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardInput = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $process = [Diagnostics.Process]::Start($info)
    return @{ Process=$process; Out=$process.StandardOutput.ReadToEndAsync(); Err=$process.StandardError.ReadToEndAsync() }
}
function Save-Logs($Run, [string]$Prefix) {
    [IO.File]::WriteAllText($Prefix + '.stdout.log', $Run.Out.GetAwaiter().GetResult())
    [IO.File]::WriteAllText($Prefix + '.stderr.log', $Run.Err.GetAwaiter().GetResult())
}

$metadata = [ordered]@{
    date_utc=[DateTime]::UtcNow.ToString('o'); seed=20261120; resolution=@(1280,720)
    frames=$Frames; repetitions=$Repetitions; label=$Label
    cpu=@(Get-CimInstance Win32_Processor | Select-Object Name,NumberOfCores,NumberOfLogicalProcessors)
    gpu=@(Get-CimInstance Win32_VideoController | Select-Object Name,DriverVersion)
    memory_bytes=(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory
    os=(Get-CimInstance Win32_OperatingSystem).Caption
    server_sha256=(Get-FileHash -LiteralPath $Server -Algorithm SHA256).Hash
    client_sha256=(Get-FileHash -LiteralPath $Client -Algorithm SHA256).Hash
    server=$Server; client=$Client; scratch=$scratch
    method='Two simultaneous real Macroquad clients over loopback UDP; opposing circular flights; 120 frames warmup; visible peer capture; polling RSS every 250 ms; CPU seconds / elapsed / logical processors.'
}
$metadata | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $Output "$Label-machine.json") -Encoding utf8
for ($repeat=1; $repeat -le $Repetitions; $repeat++) {
    $prefix = Join-Path $Output "$Label-$repeat"
    $runRoot = Join-Path $scratch "$repeat"
    [void][IO.Directory]::CreateDirectory($runRoot)
    $port = 47900 + $repeat
    $serverRun = Start-Primal $Server @('--bind', "127.0.0.1:$port", '--world', (Join-Path $runRoot 'world.pvx'))
    $clients = @()
    $peakRss = 0L
    $started = [DateTime]::UtcNow
    try {
        Start-Sleep -Milliseconds 700
        if ($serverRun.Process.HasExited) { throw 'Server exited during startup' }
        foreach ($id in @('a','b')) {
            $clients += Start-Primal $Client @('--connect', "127.0.0.1:$port", '--identity', (Join-Path $runRoot "$id.identity"), '--net-frames', "$Frames", '--net-report', "$prefix-$id.json", '--capture', "$prefix-$id.png", '--resolution', '1280x720', '--render-radius', '4')
        }
        while (@($clients | Where-Object { !$_.Process.HasExited }).Count -gt 0) {
            $serverRun.Process.Refresh()
            if ($serverRun.Process.HasExited) { throw 'Server exited during graphical route' }
            $peakRss = [Math]::Max($peakRss, $serverRun.Process.WorkingSet64)
            if (([DateTime]::UtcNow - $started).TotalSeconds -gt 120) { throw 'Graphical route timed out' }
            Start-Sleep -Milliseconds 250
        }
        $elapsed = ([DateTime]::UtcNow - $started).TotalSeconds
        $serverRun.Process.Refresh()
        $cpu = $serverRun.Process.TotalProcessorTime.TotalSeconds
        $processMetrics = [ordered]@{elapsed_seconds=$elapsed; server_cpu_seconds=$cpu; server_cpu_percent_machine=100*$cpu/$elapsed/[Environment]::ProcessorCount; server_peak_sampled_rss_bytes=$peakRss; client_exit_codes=@($clients | ForEach-Object { $_.Process.ExitCode })}
        $processMetrics | ConvertTo-Json | Set-Content -LiteralPath "$prefix-process.json" -Encoding utf8
        if (@($clients | Where-Object {$_.Process.ExitCode -ne 0}).Count -gt 0) { throw 'A graphical client failed' }
        foreach ($id in @('a','b')) {
            $report = Get-Content -LiteralPath "$prefix-$id.json" -Raw | ConvertFrom-Json
            if ($report.max_peers -lt 1 -or $report.max_visible_peers -lt 1 -or $report.changed_peer_poses -lt 1 -or $report.maximum_quads -lt 1) { throw "Missing multiplayer evidence for client $id" }
        }
    } finally {
        foreach ($clientRun in $clients) {
            if (!$clientRun.Process.HasExited) { $clientRun.Process.Kill(); $clientRun.Process.WaitForExit() }
        }
        if (!$serverRun.Process.HasExited) { $serverRun.Process.StandardInput.WriteLine('stop'); $serverRun.Process.StandardInput.Flush(); if (!$serverRun.Process.WaitForExit(20000)) { $serverRun.Process.Kill(); $serverRun.Process.WaitForExit() } }
        Save-Logs $serverRun "$prefix-server"
        for ($i=0; $i -lt $clients.Count; $i++) { Save-Logs $clients[$i] "$prefix-client-$i" }
    }
    Write-Output "PASS graphics $Label repetition $repeat"
}
