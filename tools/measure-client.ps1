# PowerShell 5.1+ on Windows. Run a single release binary at one resolution.
param(
    [Parameter(Mandatory=$true)][string]$Executable,
    [ValidatePattern('^\d+x\d+$')][string]$Resolution = '1280x720',
    [ValidateSet('faces','greedy')][string]$Mesher = 'greedy',
    [ValidateRange(240,20000)][int]$Frames = 1320,
    [string]$OutputDirectory = (Join-Path $PSScriptRoot '../benchmarks/results/local'),
    [string]$Scenario,
    [switch]$EditRoute,
    [switch]$ValleyOnly,
    [switch]$Profile,
    [switch]$Capture
)
$ErrorActionPreference = 'Stop'
$exe = (Resolve-Path -LiteralPath $Executable).Path
$out = [IO.Directory]::CreateDirectory((Join-Path $OutputDirectory $Mesher)).FullName
$base = Join-Path $out $Resolution
$info = New-Object Diagnostics.ProcessStartInfo
$info.FileName = $exe
$info.UseShellExecute = $false
$info.CreateNoWindow = $true
$info.RedirectStandardOutput = $true
$info.RedirectStandardError = $true
$info.Arguments = "--smoke --frames $Frames --resolution $Resolution --mesher $Mesher --metrics `"$base.json`""
if ($Scenario) {
    if ($Scenario -notin @('valley','region512','region1024','traverse','teleport','edits')) { throw "Scénario inconnu" }
    $info.Arguments += " --scenario $Scenario"
}
if ($ValleyOnly) { $info.Arguments += " --valley-only" }
if ($EditRoute) { $info.Arguments += " --edit-route" }
if ($Profile) { $info.Arguments += " --profile `"$base.csv`"" }
if ($Capture) { $info.Arguments += " --capture `"$base.png`"" }
$process = [Diagnostics.Process]::Start($info)
$stdout = $process.StandardOutput.ReadToEndAsync()
$stderr = $process.StandardError.ReadToEndAsync()
$peakBytes = 0L
$watch = [Diagnostics.Stopwatch]::StartNew()
while (!$process.WaitForExit(100)) {
    $process.Refresh()
    if (!$process.HasExited) { $peakBytes = [Math]::Max($peakBytes, $process.PeakWorkingSet64) }
    if ($watch.Elapsed.TotalSeconds -gt [Math]::Max(120, $Frames / 5)) {
        $process.Kill()
        throw "Le parcours graphique a dépassé son délai."
    }
}
$log = $stdout.GetAwaiter().GetResult() + $stderr.GetAwaiter().GetResult()
[IO.File]::WriteAllText("$base.log", $log.TrimEnd() + [Environment]::NewLine)
if ($process.ExitCode -ne 0) { throw "Client terminé avec code $($process.ExitCode). Voir $base.log" }
if (!(Test-Path -LiteralPath "$base.json")) { throw "Mesure du client absente." }
if ($log -notmatch "SMOKE PASS frames=$Frames\b") { throw 'Parcours graphique incomplet.' }
$report = Get-Content -Raw -LiteralPath "$base.json" | ConvertFrom-Json
if ($report.samples -ne ($Frames - $report.warmup_frames) -or
    ($report.resolution -join 'x') -ne $Resolution -or $report.mesher -ne $Mesher) {
    throw "Résolution, nombre de frames ou mesher effectif inattendu. Voir $base.json"
}
$video = Get-CimInstance Win32_VideoController | Select-Object Name,DriverVersion,CurrentRefreshRate
$meta = [ordered]@{
    cpu = (Get-CimInstance Win32_Processor).Name.Trim()
    gpu = @($video)
    physical_ram_bytes = (Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory
    windows_build = (Get-CimInstance Win32_OperatingSystem).BuildNumber
    release_binary_sha256 = (Get-FileHash -LiteralPath $exe).Hash
    peak_working_set_bytes = $peakBytes
    process_elapsed_ms = $watch.ElapsedMilliseconds
    requested_frames = $Frames
    requested_resolution = $Resolution
}
[IO.File]::WriteAllText("$base.machine.json", ($meta | ConvertTo-Json -Depth 4))
Get-Content -Raw -LiteralPath "$base.json"
