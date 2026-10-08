# Installs docc from GitHub Releases. Usage: irm https://raw.githubusercontent.com/uxname/docc/main/install.ps1 | iex
# Env: DOCC_VERSION (tag, default latest), DOCC_INSTALL_DIR (default %LOCALAPPDATA%\docc\bin), DOCC_BASE_URL (mirror).
$ErrorActionPreference = 'Stop'
$base = if ($env:DOCC_BASE_URL) { $env:DOCC_BASE_URL } else { 'https://github.com/uxname/docc/releases' }
$dir = if ($env:DOCC_INSTALL_DIR) { $env:DOCC_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'docc\bin' }
$list = if ($env:DOCC_VERSION) { "$base/download/$($env:DOCC_VERSION)/checksums.txt" } else { "$base/latest/download/checksums.txt" }
$tmp = Join-Path ([IO.Path]::GetTempPath()) ([IO.Path]::GetRandomFileName())
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
  $text = (Invoke-WebRequest -UseBasicParsing $list).Content
  $m = [regex]::Match($text, '(?m)^([0-9a-f]{64})\s+\*?(docc_(\S+)_windows_amd64\.zip)\s*$')
  if (-not $m.Success) { throw 'the release has no archive for windows/amd64' }
  $want, $name, $tag = $m.Groups[1].Value, $m.Groups[2].Value, $m.Groups[3].Value
  $zip = Join-Path $tmp $name
  Invoke-WebRequest -UseBasicParsing "$base/download/$tag/$name" -OutFile $zip
  if ((Get-FileHash $zip -Algorithm SHA256).Hash.ToLower() -ne $want) { throw "checksum mismatch for $name, nothing installed" }
  Expand-Archive $zip -DestinationPath $tmp
  New-Item -ItemType Directory -Force -Path $dir | Out-Null
  $exe = Join-Path $dir 'docc.exe'
  if (Test-Path $exe) { Move-Item -Force $exe "$exe.old" }
  Copy-Item (Join-Path $tmp ($name -replace '\.zip$', '') | Join-Path -ChildPath 'docc.exe') $exe
  Write-Host "docc $tag installed to $exe"
  $user = [Environment]::GetEnvironmentVariable('Path', 'User')
  if (($user -split ';') -notcontains $dir) {
    [Environment]::SetEnvironmentVariable('Path', "$user;$dir", 'User')
    Write-Host "added $dir to the user PATH; open a new terminal"
  }
} finally { Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue }
