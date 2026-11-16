param(
  [Parameter(Mandatory = $true)][string] $MsiPath,
  [Parameter(Mandatory = $true)][string] $Version,
  [Parameter(Mandatory = $true)][string] $Fixture
)

$ErrorActionPreference = 'Stop'
$MsiPath = (Resolve-Path -LiteralPath $MsiPath).Path
$Fixture = (Resolve-Path -LiteralPath $Fixture).Path
$install = Start-Process -FilePath msiexec.exe -ArgumentList @('/i', "`"$MsiPath`"", '/qn', '/norestart', 'ALLUSERS=1') -Wait -PassThru
if ($install.ExitCode -ne 0) { throw "MSI installation failed with exit code $($install.ExitCode)" }

$installDir = Join-Path $env:ProgramFiles 'ArtCraft\EncoderCraft'
if (-not (Test-Path -LiteralPath $installDir)) { throw "MSI install directory not found: $installDir" }
$cli = Join-Path $installDir 'encodercraft-cli.exe'
$gui = Join-Path $installDir 'encodercraft.exe'
foreach ($file in @($cli, $gui, (Join-Path $installDir 'LICENSE-MIT'), (Join-Path $installDir 'LICENSE-APACHE'), (Join-Path $installDir 'NOTICE'))) {
  if (-not (Test-Path -LiteralPath $file -PathType Leaf)) { throw "Installed file missing: $file" }
}

$guiRun = Start-Process -FilePath $gui -ArgumentList '--version' -Wait -PassThru
if ($guiRun.ExitCode -ne 0) { throw "GUI --version exited with $($guiRun.ExitCode)" }
$cliVersion = (& $cli --version).Trim()
if ($LASTEXITCODE -ne 0 -or $cliVersion -ne "encodercraft-cli $Version") { throw "Unexpected CLI version: $cliVersion" }

$work = Join-Path ([IO.Path]::GetTempPath()) "encodercraft-installer-smoke-$([guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Path $work | Out-Null
try {
  $png = Join-Path $work 'out.png'
  & $cli encode $Fixture -p 'Still Image PNG (Lossless)' -o $png *> $null
  if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $png -PathType Leaf) -or (Get-Item -LiteralPath $png).Length -eq 0) {
    throw 'Installed CLI failed its PNG transcode smoke test'
  }
  $mp4 = Join-Path $work 'out.mp4'
  & $cli encode $Fixture -o $mp4 *> $null
  if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $mp4 -PathType Leaf) -or (Get-Item -LiteralPath $mp4).Length -le 1000) {
    throw 'Installed CLI failed its MP4 transcode smoke test'
  }
} finally {
  Remove-Item -LiteralPath $work -Recurse -Force
}

Write-Output "OK: installed MSI ($Version)"
