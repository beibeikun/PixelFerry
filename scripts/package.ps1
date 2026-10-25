param([string]$MakeNsis)
$ErrorActionPreference='Stop'
$projectRoot=Split-Path $PSScriptRoot -Parent
Push-Location (Join-Path $projectRoot 'frontend')
try { & npm run build; if ($LASTEXITCODE -ne 0) {throw 'Frontend build failed.'} } finally {Pop-Location}
& (Join-Path $PSScriptRoot 'build.ps1') -Action build -Release
$portable=Join-Path $projectRoot 'release/PixelFerry'
New-Item -ItemType Directory -Force -Path $portable | Out-Null
Copy-Item -LiteralPath (Join-Path $projectRoot 'target/release/pixelferry.exe') -Destination $portable
Copy-Item -LiteralPath (Join-Path $projectRoot 'resources/tools') -Destination $portable -Recurse -Force
Copy-Item -LiteralPath (Join-Path $projectRoot 'resources/THIRD_PARTY.md'),(Join-Path $projectRoot 'README.md') -Destination $portable
Copy-Item -LiteralPath (Join-Path $projectRoot 'Cargo.lock') -Destination $portable
Copy-Item -LiteralPath (Join-Path $projectRoot 'frontend/package-lock.json'),(Join-Path $projectRoot 'resources/MicrosoftEdgeWebview2Setup.exe') -Destination $portable
Copy-Item -LiteralPath (Join-Path $projectRoot 'docs') -Destination $portable -Recurse -Force
foreach ($runtimeName in @('vcruntime140.dll','vcruntime140_1.dll','msvcp140.dll','msvcp140_1.dll','msvcp140_2.dll','msvcp140_atomic_wait.dll')) {
    $runtimeSource=Join-Path $env:WINDIR "System32/$runtimeName"
    if (!(Test-Path -LiteralPath $runtimeSource)) { throw "Missing app-local Microsoft runtime: $runtimeName" }
    Copy-Item -LiteralPath $runtimeSource -Destination $portable
}
$licenseDirectory=Join-Path $portable 'licenses'
New-Item -ItemType Directory -Force -Path $licenseDirectory | Out-Null
$crateSources=Join-Path $env:USERPROFILE '.cargo/registry/src'
foreach ($crate in (Get-ChildItem $crateSources -Directory | Get-ChildItem -Directory)) {
    $notices=@(Get-ChildItem -LiteralPath $crate.FullName -File | Where-Object Name -Match '^(LICENSE|COPYING|NOTICE|UNLICENSE)')
    if ($notices.Count -gt 0) {
        $crateDestination=Join-Path $licenseDirectory $crate.Name
        New-Item -ItemType Directory -Force -Path $crateDestination | Out-Null
        foreach ($noticeFile in $notices) { Copy-Item -LiteralPath $noticeFile.FullName -Destination $crateDestination }
    }
}
$skiaNotices=Get-ChildItem (Join-Path $projectRoot 'target/release/build') -Recurse -Filter 'LICENSE*' | Where-Object FullName -Match 'skia'
foreach ($noticeFile in $skiaNotices) { Copy-Item -LiteralPath $noticeFile.FullName -Destination (Join-Path $portable 'Skia-LICENSE.txt') }
$jsPackages=(Get-Content (Join-Path $projectRoot 'frontend/package-lock.json') -Raw | ConvertFrom-Json -AsHashtable).packages
foreach ($packagePath in $jsPackages.Keys) {
    if (!$packagePath.StartsWith('node_modules/')) {continue}
    $packageRoot=Join-Path (Join-Path $projectRoot 'frontend') $packagePath
    $jsNotices=@(Get-ChildItem -LiteralPath $packageRoot -File -ErrorAction SilentlyContinue | Where-Object Name -Match '^(LICENSE|COPYING|NOTICE|UNLICENSE)')
    if ($jsNotices.Count -gt 0) {
        $jsDestination=Join-Path $licenseDirectory ('npm-'+($packagePath -replace 'node_modules/','' -replace '[/@]','_'))
        New-Item -ItemType Directory -Force -Path $jsDestination | Out-Null
        foreach ($notice in $jsNotices) {Copy-Item -LiteralPath $notice.FullName -Destination $jsDestination}
    }
}
Compress-Archive -LiteralPath $portable -DestinationPath (Join-Path $projectRoot 'release/PixelFerry-0.2.0-win-x64.zip') -Force
if (!$MakeNsis) {
    $candidate=Get-ChildItem (Join-Path $env:LOCALAPPDATA 'electron-builder/Cache/nsis-*') -Recurse -Filter makensis.exe -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($candidate) { $MakeNsis=$candidate.FullName }
}
if (!$MakeNsis) { throw 'Portable ZIP created; supply -MakeNsis path to build installer.' }
& $MakeNsis "/DPROJECT_ROOT=$projectRoot" (Join-Path $PSScriptRoot 'installer.nsi')
if ($LASTEXITCODE -ne 0) { throw 'NSIS packaging failed.' }
Get-FileHash -Algorithm SHA256 (Join-Path $projectRoot 'release/PixelFerry-0.2.0-win-x64.zip'),(Join-Path $projectRoot 'release/PixelFerry-Setup-0.2.0.exe') | Format-Table
