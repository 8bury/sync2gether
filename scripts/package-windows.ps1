param(
    [string]$LibMpvPath = $env:LIBMPV_DLL
)

$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

$cargoToml = Get-Content -Raw Cargo.toml
if ($cargoToml -notmatch '(?m)^version = "([^"]+)"') {
    throw "Não foi possível ler a versão em Cargo.toml"
}
$version = $Matches[1]

$targetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { "target" }
$binary = Join-Path $targetDir "release\sync2gether.exe"
if (-not (Test-Path $binary)) {
    throw "Binário não encontrado: $binary"
}
if (-not $LibMpvPath -or -not (Test-Path $LibMpvPath)) {
    throw "libmpv-2.dll não encontrada. Informe -LibMpvPath ou LIBMPV_DLL."
}

$distDir = Join-Path $PWD "target\dist"
$workDir = Join-Path $PWD "target\packaging\windows"
$stageName = "sync2gether-$version-windows-x86_64"
$stageDir = Join-Path $workDir $stageName

if (Test-Path $workDir) {
    Remove-Item -Recurse -Force $workDir
}
New-Item -ItemType Directory -Force -Path $distDir, $stageDir | Out-Null

Copy-Item $binary (Join-Path $stageDir "sync2gether.exe")
Copy-Item $LibMpvPath (Join-Path $stageDir "libmpv-2.dll")
Copy-Item "LICENSE" (Join-Path $stageDir "LICENSE")

$stagedExe = Join-Path $stageDir "sync2gether.exe"
$actualVersion = (& $stagedExe --version | Out-String).Trim()
if ($actualVersion -ne "sync2gether $version") {
    throw "Versão inesperada: '$actualVersion' (esperado 'sync2gether $version')"
}
$helpText = (& $stagedExe --help | Out-String)
if ($helpText -match "--demo") {
    throw "Binário de release não deve incluir a feature demo"
}

$zipOut = Join-Path $distDir "$stageName.zip"
if (Test-Path $zipOut) {
    Remove-Item -Force $zipOut
}
Compress-Archive -Path $stageDir -DestinationPath $zipOut -CompressionLevel Optimal
if (-not (Test-Path $zipOut)) {
    throw "Falha ao gerar $zipOut"
}

$setupOut = Join-Path $distDir "$stageName-setup.exe"
if (Test-Path $setupOut) {
    Remove-Item -Force $setupOut
}
$makensis = Get-Command makensis -ErrorAction SilentlyContinue
if (-not $makensis) {
    $fallback = "C:\Program Files (x86)\NSIS\makensis.exe"
    if (Test-Path $fallback) {
        $makensis = $fallback
    } else {
        throw "makensis não encontrado para gerar o instalador NSIS."
    }
}
& $makensis "/DVERSION=$version" "/DSTAGE_DIR=$stageDir" "/DOUT_FILE=$setupOut" "packaging\windows\installer.nsi"
if ($LASTEXITCODE -ne 0 -or -not (Test-Path $setupOut)) {
    throw "Falha ao gerar o instalador NSIS"
}

Write-Host "Gerados:"
Write-Host "  $zipOut"
Write-Host "  $setupOut"
