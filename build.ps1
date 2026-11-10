param(
    [switch]$Clean
)

$ErrorActionPreference = "Stop"
$ProjectRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $ProjectRoot

$VenvPython = Join-Path $ProjectRoot ".venv314\Scripts\python.exe"
if (-not (Test-Path -LiteralPath $VenvPython)) {
    $VenvPython = Join-Path $ProjectRoot ".venv\Scripts\python.exe"
}
if (-not (Test-Path -LiteralPath $VenvPython)) {
    throw "Create a project virtual environment and install the [desktop] extra first."
}

# PyInstaller resolves DLL dependencies through PATH. Tool runtimes (e.g. poppler or
# libheif bundles) can put their own icuuc.dll / ucrtbase.dll on PATH; PyInstaller
# then bundles those instead of the Windows system copies, and Qt6Core.dll fails at
# startup with "DLL load failed while importing QtCore: The specified procedure could
# not be found". Build with a minimal PATH so only system DLLs are found.
$VenvScripts = Split-Path -Parent $VenvPython
$env:PATH = @(
    $VenvScripts,
    (Join-Path $env:SystemRoot "System32"),
    $env:SystemRoot,
    (Join-Path $env:SystemRoot "System32\Wbem")
) -join ";"

$PyInstallerArgs = @("-m", "PyInstaller", "--noconfirm")
if ($Clean) {
    $PyInstallerArgs += "--clean"
}
$PyInstallerArgs += @(
    "--windowed", "--name", "Tapo Desktop",
    "--hidden-import", "keyring.backends.Windows",
    "app\main.py"
)

& $VenvPython @PyInstallerArgs
if ($LASTEXITCODE -ne 0) {
    throw "PyInstaller failed with exit code $LASTEXITCODE"
}

# Qt6Core.dll must use Windows' own icuuc.dll from System32. A bundled ICU copy
# exports versioned symbols (ucnv_open_78, ...) and breaks the QtCore import.
$Internal = Join-Path $ProjectRoot "dist\Tapo Desktop\_internal"
$Foreign = Get-ChildItem -LiteralPath $Internal -File |
    Where-Object { $_.Name -match '^(icu.*|ucrtbase)\.dll$' }
if ($Foreign) {
    throw "Bundle contains non-system DLLs that break Qt: $($Foreign.Name -join ', ')"
}
Write-Host "Built: $ProjectRoot\dist\Tapo Desktop\Tapo Desktop.exe"
