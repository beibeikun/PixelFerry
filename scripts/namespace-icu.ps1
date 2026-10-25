param([Parameter(Mandatory)][string]$Profile)
$ErrorActionPreference='Stop'
$projectRoot=Split-Path $PSScriptRoot -Parent
$rustc=(& (Join-Path $env:USERPROFILE '.cargo/bin/rustup.exe') which rustc)
$toolchainRoot=Split-Path (Split-Path $rustc -Parent) -Parent
$llvmTools=Join-Path $toolchainRoot 'lib/rustlib/x86_64-pc-windows-msvc/bin'
$nm=Join-Path $llvmTools 'llvm-nm.exe'
$objcopy=Join-Path $llvmTools 'llvm-objcopy.exe'
if (!(Test-Path $objcopy)) { throw 'Run rustup component add llvm-tools before building PixelFerry.' }
# Skia 0.84 ships unversioned ICU C exports. Windows globalization imports the
# same names. Namespace Skia's native definitions AND all its native references;
# do not suppress duplicate symbols or change the registry/crate source cache.
$icuLibraries=Get-ChildItem (Join-Path $projectRoot "target/$Profile/build/skia-bindings-*/out/skia/skunicode_icu.lib")
foreach ($icuLibrary in $icuLibraries) {
    $symbols=& $nm --defined-only --extern-only --format=posix $icuLibrary.FullName
    if ($LASTEXITCODE -ne 0) { throw 'Unable to inspect Skia ICU exports.' }
    $mapping=@($symbols | Where-Object { $_ -match '^((?:u[a-z0-9]*_|utf8_|icu_)[A-Za-z0-9_]+)\s+[TDRB]\s' } | ForEach-Object { $symbol=($_ -split '\s+')[0]; "$symbol pixelferry_$symbol" } | Sort-Object -Unique)
    if ($mapping.Count -eq 0) { continue }
    $mappingPath=Join-Path $icuLibrary.DirectoryName 'pixelferry-icu-symbols.txt'
    $mapping | Set-Content -LiteralPath $mappingPath -Encoding ascii
    foreach ($nativeLibrary in (Get-ChildItem -LiteralPath $icuLibrary.DirectoryName -Filter '*.lib')) {
        $temporary=$nativeLibrary.FullName+'.pixelferry'
        & $objcopy "--redefine-syms=$mappingPath" $nativeLibrary.FullName $temporary
        if ($LASTEXITCODE -ne 0) { throw "Unable to namespace $($nativeLibrary.Name)" }
        Move-Item -LiteralPath $temporary -Destination $nativeLibrary.FullName -Force
    }
    Write-Output "Namespaced $($mapping.Count) Skia ICU symbols for $Profile."
}
