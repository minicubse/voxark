# Dot-source from PowerShell, then use cargo normally in this shell.
param([string]$Cache = (Join-Path $PSScriptRoot '../.tools'), [switch]$Install)
$ErrorActionPreference = 'Stop'
$Cache = [IO.Path]::GetFullPath($Cache)
$rustVersion = '1.100.0'
$hostTriple = 'x86_64-pc-windows-gnu'
$toolDir = 'llvm-mingw-20261117-ucrt-x86_64'
$env:CARGO_HOME = Join-Path $Cache 'cargo'
$env:RUSTUP_HOME = Join-Path $Cache 'rustup'
$llvm = Join-Path $Cache $toolDir
$rustup = Join-Path $env:CARGO_HOME 'bin/rustup.exe'
function Get-PrimalArchive([string]$Url, [string]$Path, [string]$Digest) {
    if (!(Test-Path -LiteralPath $Path) -or (Get-FileHash -LiteralPath $Path).Hash -ne $Digest) {
        & curl.exe -fLsS --retry 2 $Url -o $Path
        if ($LASTEXITCODE -ne 0) { throw "Téléchargement impossible : $Url" }
    }
    if ((Get-FileHash -LiteralPath $Path).Hash -ne $Digest) { throw "SHA-256 incorrect : $Path" }
}
if ($Install) {
    [void][IO.Directory]::CreateDirectory($Cache)
    if (!(Test-Path -LiteralPath $rustup)) {
        $installer = Join-Path $Cache 'rustup-init.exe'
        Get-PrimalArchive "https://static.rust-lang.org/rustup/archive/1.29.3/$hostTriple/rustup-init.exe" $installer 'F6289656CE42E1099F3CBC6F10EF6321DFDE54773FAF599DD36FD3B1F07D9103'
        & $installer -y --no-modify-path --default-host $hostTriple --default-toolchain $rustVersion --profile minimal --component rustfmt --component clippy
        if ($LASTEXITCODE -ne 0) { throw 'Installation Rust incomplète.' }
    }
    if (!(Test-Path -LiteralPath (Join-Path $llvm 'x86_64-w64-mingw32/lib/libimm32.a'))) {
        $zip = Join-Path $Cache "$toolDir.zip"
        Get-PrimalArchive "https://github.com/mstorsjo/llvm-mingw/releases/download/20261117/$toolDir.zip" $zip '4D002E330A0F703625AA5CCBC37E96F8F824413E163BFC7336774AABD2C3A412'
        & tar.exe -xf $zip -C $Cache
        if ($LASTEXITCODE -ne 0) { throw 'Extraction LLVM-MinGW incomplète.' }
    }
}
if (!(Test-Path -LiteralPath $rustup) -or !(Test-Path -LiteralPath $llvm)) {
    throw 'Relancer ce script avec -Install pour préparer ce cache local.'
}
$env:RUSTUP_TOOLCHAIN = "$rustVersion-$hostTriple"
$stableCompiler = Join-Path $env:RUSTUP_HOME "toolchains/stable-$hostTriple/bin/rustc.exe"
if ((Test-Path -LiteralPath $stableCompiler) -and (& $stableCompiler -V) -match '^rustc 1\.100\.0 \(0ef1bc386 ') {
    $env:RUSTUP_TOOLCHAIN = "stable-$hostTriple"
} elseif ($Install) {
    & $rustup toolchain install "$rustVersion-$hostTriple" --profile minimal --component clippy --component rustfmt
    if ($LASTEXITCODE -ne 0) { throw 'Chaîne Rust incomplète.' }
}
$env:LIBRARY_PATH = Join-Path $llvm 'x86_64-w64-mingw32/lib'
$env:Path = "$env:CARGO_HOME\bin;$llvm\bin;$env:Path"
