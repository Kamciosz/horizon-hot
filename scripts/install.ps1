# PL: Instalacja aplikacji gracza hot-play na Windows (M11.3, ADR 0016). W PowerShellu:
#   irm https://github.com/Kamciosz/horizon-hot/releases/download/hot-play/install.ps1 -OutFile $env:TEMP\hot-play-install.ps1
#   powershell -ExecutionPolicy Bypass -File $env:TEMP\hot-play-install.ps1 ADRES
# Plik ma BOM UTF-8, bo PowerShell 5.1 czyta plik bez BOM jako ANSI i psuje polskie znaki.
# Pobiera plik z wydania `hot-play`, sprawdza SHA-256, zapisuje adres serwera i robi skrót „Graj”
# na pulpicie. Ponowne uruchomienie jest bezpieczne: plik trafia na miejsce dopiero po sprawdzeniu.
# EN: Installs the hot-play player app on Windows (M11.3, ADR 0016). Downloads the file from the
# `hot-play` release, checks SHA-256, saves the server address and makes a "Graj" desktop
# shortcut. Running it again is safe: the file is moved in only after the checksum matches.
param([string]$Adres = "")

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

$base = if ($env:HOT_PLAY_BASE) { $env:HOT_PLAY_BASE } else { "https://github.com/Kamciosz/horizon-hot/releases/download/hot-play" }
$asset = "hot-play-windows-x64.exe"
$data = if ($env:HOT_PLAY_HOME) { $env:HOT_PLAY_HOME } else { Join-Path $env:LOCALAPPDATA "hot-play" }
$exe = Join-Path $data "hot-play.exe"
$tmp = Join-Path $data "hot-play.exe.download"

New-Item -ItemType Directory -Force -Path $data | Out-Null
Write-Host "Pobieram hot-play ($asset)..."
$headers = @{ "User-Agent" = "hot-play" }
$sums = Invoke-WebRequest -UseBasicParsing -Headers $headers -Uri "$base/SHA256SUMS"
$text = if ($sums.Content -is [byte[]]) { [Text.Encoding]::UTF8.GetString($sums.Content) } else { $sums.Content }
$want = $null
foreach ($line in ($text -split "`n")) {
    $parts = $line.Trim() -split "\s+"
    if ($parts.Count -eq 2 -and ($parts[1] -eq $asset -or $parts[1] -eq "*$asset")) { $want = $parts[0].ToLower() }
}
if (-not $want) { throw "SHA256SUMS nie zna pliku $asset." }
Invoke-WebRequest -UseBasicParsing -Headers $headers -Uri "$base/$asset" -OutFile $tmp
$got = (Get-FileHash -Algorithm SHA256 -Path $tmp).Hash.ToLower()
if ($got -ne $want) {
    Remove-Item -Force $tmp
    throw "Pobrany plik ma inną sumę SHA-256 niż w wydaniu (przerwane pobieranie?). Uruchom instalację jeszcze raz."
}
Move-Item -Force -Path $tmp -Destination $exe
& $exe --version | Out-Null
if ($LASTEXITCODE -ne 0) { throw "Pobrany hot-play się nie uruchamia." }
Write-Host "Zainstalowany: $exe"

if ($Adres) {
    $env:HOT_PLAY_NO_UPDATE = "1"
    & $exe ustaw $Adres
    Remove-Item Env:HOT_PLAY_NO_UPDATE
}

$desktop = [Environment]::GetFolderPath("Desktop")
$link = (New-Object -ComObject WScript.Shell).CreateShortcut((Join-Path $desktop "Graj.lnk"))
$link.TargetPath = $exe
$link.WorkingDirectory = $data
$link.Description = "Minecraft Horizon na serwerze horizon-hot"
$link.Save()
Write-Host "Skrót: Graj na pulpicie (dwuklik)."
Write-Host "Gotowe."
