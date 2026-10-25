param([ValidateSet('check','test','build','clippy')][string]$Action='build',[switch]$Release)
$ErrorActionPreference='Stop'
$projectRoot=Split-Path $PSScriptRoot -Parent
$resourceCompiler=Get-ChildItem (Join-Path $projectRoot '.tools/sdk-buildtools/bin/*/x64/rc.exe') -ErrorAction SilentlyContinue | Select-Object -First 1
if ($resourceCompiler) { $env:PATH=$resourceCompiler.DirectoryName+';'+$env:PATH }
$cppCompiler=Get-ChildItem (Join-Path $projectRoot '.tools/Contents/VC/Tools/MSVC/*/bin/Hostx64/x64/cl.exe') -ErrorAction SilentlyContinue | Select-Object -First 1
if ($cppCompiler) {
    $env:PATH=$cppCompiler.DirectoryName+';'+$env:PATH
    $env:CC=$cppCompiler.FullName
    $env:CXX=$cppCompiler.FullName
    $env:AR=Join-Path $cppCompiler.DirectoryName 'lib.exe'
    $includeCandidates=@('.tools/windows-sdk/crt/include','.tools/windows-sdk/sdk/include/um','.tools/windows-sdk/sdk/include/shared','.tools/windows-sdk/sdk/include/winrt','.tools/windows-sdk/sdk/include/ucrt')
    $env:INCLUDE=(@($includeCandidates | ForEach-Object {Join-Path $projectRoot $_} | Where-Object {Test-Path $_}) -join ';')
}
if (!(Test-Path (Join-Path $projectRoot 'frontend/dist/index.html'))) {
    Push-Location (Join-Path $projectRoot 'frontend')
    try { & npm run build; if ($LASTEXITCODE -ne 0) {throw 'Frontend build failed.'} } finally {Pop-Location}
}
$cargoExecutable=Join-Path $env:USERPROFILE '.cargo/bin/cargo.exe'
if (!(Test-Path $cargoExecutable)) { throw 'Install Rust with rustup first.' }
$sdkRoot=Join-Path $projectRoot '.tools/windows-sdk'
if (Test-Path $sdkRoot) {
    $hostToolchain=(& (Join-Path $env:USERPROFILE '.cargo/bin/rustup.exe') which rustc)
    $hostRoot=Split-Path (Split-Path $hostToolchain -Parent) -Parent
    $env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER=Join-Path $hostRoot 'lib/rustlib/x86_64-pc-windows-msvc/bin/rust-lld.exe'
    $libraryPaths=Get-ChildItem -LiteralPath $sdkRoot -Filter '*.lib' -Recurse | ForEach-Object DirectoryName | Sort-Object -Unique
    if (!$libraryPaths) { throw 'Portable Windows SDK download is incomplete.' }
    $env:LIB=($libraryPaths -join ';')
}
$cargoArguments=@($Action,'--manifest-path',(Join-Path $projectRoot 'Cargo.toml'),'--workspace')
if ($Release) { $cargoArguments+='--release' }
Push-Location $projectRoot
try {
    if ($Action -ne 'check') {
        # Resolve the complete workspace feature graph before patching Skia;
        # Tauri enables shared dependencies that change the native build hash.
        $nativeArguments=@('check','--workspace')
        if ($Release) { $nativeArguments+='--release' }
        & $cargoExecutable @nativeArguments
        if ($LASTEXITCODE -ne 0) { throw 'Native renderer preparation failed.' }
        & (Join-Path $PSScriptRoot 'namespace-icu.ps1') -Profile $(if ($Release) {'release'} else {'debug'})
    }
    & $cargoExecutable @cargoArguments
    if ($LASTEXITCODE -ne 0) { throw "cargo $Action failed ($LASTEXITCODE)" }
} finally { Pop-Location }
