param([string]$FfmpegDirectory)
$ErrorActionPreference='Stop'
$projectRoot=Split-Path $PSScriptRoot -Parent
$bootstrapper=Join-Path $projectRoot 'resources/MicrosoftEdgeWebview2Setup.exe'
if (!(Test-Path $bootstrapper)) {
    & curl.exe -sSfL --connect-timeout 15 --max-time 120 -o $bootstrapper 'https://go.microsoft.com/fwlink/p/?LinkId=2124703'
    if ($LASTEXITCODE -ne 0) {throw 'WebView2 bootstrapper download failed.'}
}
$signature=Get-AuthenticodeSignature -LiteralPath $bootstrapper
if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch 'CN=Microsoft Corporation,') {throw 'WebView2 bootstrapper signature is invalid.'}
if (!$FfmpegDirectory) { $FfmpegDirectory=Join-Path (Split-Path $projectRoot -Parent) 'lg600-control/resources' }
$destination=Join-Path $projectRoot 'resources/tools'
New-Item -ItemType Directory -Force -Path $destination | Out-Null
foreach ($name in @('ffmpeg.exe','ffprobe.exe')) {
    $source=Join-Path $FfmpegDirectory $name
    if (!(Test-Path -LiteralPath $source)) { throw "Missing $source" }
    Copy-Item -LiteralPath $source -Destination (Join-Path $destination $name)
}
foreach ($name in @('FFmpeg-license.txt','FFmpeg-sha256.txt')) {
    $source=Join-Path $FfmpegDirectory $name
    if (Test-Path -LiteralPath $source) { Copy-Item -LiteralPath $source -Destination (Join-Path $destination $name) }
}
Get-FileHash -Algorithm SHA256 (Join-Path $destination 'ffmpeg.exe'),(Join-Path $destination 'ffprobe.exe') | Format-Table
