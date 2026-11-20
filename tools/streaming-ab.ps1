# One process at a time. Keep each run's raw timings and machine metadata.
param(
    [Parameter(Mandatory=$true)][string]$Control,
    [Parameter(Mandatory=$true)][string]$Predictive,
    [string]$Output = (Join-Path $PSScriptRoot '../benchmarks/results/1c/ab')
)
$ErrorActionPreference='Stop'
$cases=@(
    @('walk',480,2), @('sprint',480,2), @('fly',480,2),
    @('traverse',840,2), @('teleport',480,2), @('return',720,2),
    @('moving-edits',480,2), @('long-flight',1560,2),
    @('traverse',840,4), @('traverse',840,8)
)
for ($repeat=1;$repeat -le 3;$repeat++) {
    $index=0
    foreach ($case in $cases) {
        $scenario,$frames,$radius=$case
        $order=if (($index+$repeat)%2 -eq 1) { @('A','B') } else { @('B','A') }
        foreach ($variant in $order) {
            $exe=if ($variant -eq 'A') { $Control } else { $Predictive }
            $out=Join-Path $Output "$scenario-r$radius/$repeat-$variant"
            & "$PSScriptRoot/measure-client.ps1" -Executable $exe -Scenario $scenario -Frames $frames -PreloadRadius $radius -Profile -OutputDirectory $out | Out-Null
            Write-Output "$scenario radius=$radius repeat=$repeat $variant OK"
        }
        $index++
    }
}
