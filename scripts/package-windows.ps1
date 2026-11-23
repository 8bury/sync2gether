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
$stagedDll = Join-Path $stageDir "libmpv-2.dll"
$actualVersion = (& $stagedExe --version | Out-String).Trim()
if ($LASTEXITCODE -ne 0 -or $actualVersion -ne "sync2gether $version") {
    throw "Versão inesperada: '$actualVersion' (esperado 'sync2gether $version')"
}
$helpText = (& $stagedExe --help | Out-String)
if ($LASTEXITCODE -ne 0 -or $helpText -match "--demo") {
    throw "Binário de release não deve incluir a feature demo"
}

Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;

public static class LibMpvProbe {
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern IntPtr LoadLibrary(string lpFileName);

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern IntPtr GetProcAddress(IntPtr hModule, string procName);

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool FreeLibrary(IntPtr hModule);

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public delegate uint ApiVersionDelegate();

    public static uint Probe(string dllPath) {
        IntPtr handle = LoadLibrary(dllPath);
        if (handle == IntPtr.Zero) {
            throw new Exception("LoadLibrary falhou com Win32 error " + Marshal.GetLastWin32Error());
        }
        try {
            IntPtr proc = GetProcAddress(handle, "mpv_client_api_version");
            if (proc == IntPtr.Zero) {
                throw new Exception("Símbolo mpv_client_api_version não encontrado");
            }
            var fn = Marshal.GetDelegateForFunctionPointer<ApiVersionDelegate>(proc);
            return fn();
        } finally {
            FreeLibrary(handle);
        }
    }
}
"@
$apiVersion = [LibMpvProbe]::Probe($stagedDll)
$major = $apiVersion -shr 16
$minor = $apiVersion -band 0xFFFF
if ($major -lt 2) {
    throw "Versão da API libmpv inesperada: $major.$minor"
}
Write-Host "libmpv-2.dll carregada com sucesso (API $major.$minor)."

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

$testInstallDir = Join-Path $workDir "installed-smoke"
$installProc = Start-Process -FilePath $setupOut -ArgumentList "/S", "/D=$testInstallDir" -Wait -PassThru
if ($installProc.ExitCode -ne 0) {
    throw "Instalação silenciosa NSIS falhou (exit $($installProc.ExitCode))"
}
$installedExe = Join-Path $testInstallDir "sync2gether.exe"
$installedDll = Join-Path $testInstallDir "libmpv-2.dll"
$uninstaller = Join-Path $testInstallDir "uninstall.exe"
if (-not (Test-Path $installedExe) -or -not (Test-Path $installedDll) -or -not (Test-Path $uninstaller)) {
    throw "Arquivos esperados ausentes após instalação NSIS em $testInstallDir"
}
$installedVer = (& $installedExe --version | Out-String).Trim()
if ($LASTEXITCODE -ne 0 -or $installedVer -ne "sync2gether $version") {
    throw "Binário instalado retornou versão inesperada: '$installedVer'"
}
$uninstallProc = Start-Process -FilePath $uninstaller -ArgumentList "/S", "_?=$testInstallDir" -Wait -PassThru
if ($uninstallProc.ExitCode -ne 0) {
    throw "Desinstalação silenciosa NSIS falhou (exit $($uninstallProc.ExitCode))"
}
Remove-Item -Recurse -Force $testInstallDir -ErrorAction SilentlyContinue

Write-Host "Gerados e validados:"
Write-Host "  $zipOut"
Write-Host "  $setupOut"
