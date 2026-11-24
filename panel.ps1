$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
try { [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12 } catch { }
$UA = 'EasyMinecraftServer/2.0 (https://example.com/contact)'
$root = if ($env:BH_ROOT) { $env:BH_ROOT } else { Join-Path $env:USERPROFILE 'MinecraftServer' }
$sd = Join-Path $root 'server'
$bk = Join-Path $root 'backups'
$cfgPath = Join-Path $root 'settings.json'
$stopFlag = Join-Path $root 'stop.flag'
$script:JsonTimeout = 100
$ModrinthBase = if ($env:BH_MODRINTH) { $env:BH_MODRINTH } else { 'https://api.modrinth.com/v2' }
$RconPort = if ($env:BH_RCON_PORT) { [int]$env:BH_RCON_PORT } else { 25575 }

function Say($m) { Write-Host $m -ForegroundColor Green }
function Warn($m) { Write-Host $m -ForegroundColor Yellow }
function Bad($m) { Write-Host $m -ForegroundColor Red }
function Wait-Enter { [void](Read-Host 'Press Enter to continue') }
function Get-Json($url) { Invoke-RestMethod -Uri $url -UserAgent $UA -TimeoutSec $script:JsonTimeout }
function Get-File($url, $dest) { Write-Host '  downloading...'; Invoke-WebRequest -Uri $url -OutFile $dest -UserAgent $UA }
function Test-Name($n) { return ($n -match '^[A-Za-z0-9_]{3,16}$') }
function Stamp { Get-Date -Format 'yyyyMMdd-HHmmss' }
function GB($mb) { return [math]::Round($mb / 1024, 1) }

# ---------- this PC ----------
function Get-Hardware {
  $ram = if ($env:BH_RAM) { [int]$env:BH_RAM } else { [int][math]::Floor((Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory / 1MB) }
  $reserve = if ($ram -le 4096) { 1536 } else { 2048 }
  $h = [math]::Min($ram - $reserve, [math]::Floor($ram * 0.6))
  $h = [int][math]::Max(1024, [math]::Min(12288, $h))
  if ($h -lt 1536) { $p = 4; $v = 6; $s = 4 } elseif ($h -lt 3072) { $p = 8; $v = 8; $s = 6 } elseif ($h -lt 6144) { $p = 16; $v = 10; $s = 8 } else { $p = 32; $v = 12; $s = 10 }
  return @{ ram = $ram; heap = $h; players = $p; view = $v; sim = $s }
}

# ---------- settings ----------
function Load-Settings {
  $h = @{}
  if (Test-Path $cfgPath) {
    $o = Get-Content $cfgPath -Raw | ConvertFrom-Json
    foreach ($p in $o.PSObject.Properties) { $h[$p.Name] = $p.Value }
  }
  return $h
}
function Save-Settings { $script:S | ConvertTo-Json | Set-Content -Path $cfgPath -Encoding ASCII }
function New-Password { return (-join ((48..57) + (65..90) + (97..122) | Get-Random -Count 24 | ForEach-Object { [char]$_ })) }
function Ensure-Defaults {
  $changed = $false
  if (-not $script:S.ContainsKey('heapMb')) { $script:S['heapMb'] = $script:HW.heap; $changed = $true }
  if (-not $script:S.ContainsKey('rconPassword')) { $script:S['rconPassword'] = New-Password; $changed = $true }
  if (-not $script:S.ContainsKey('tunnel')) { $script:S['tunnel'] = $false; $changed = $true }
  if (-not $script:S.ContainsKey('backupHours')) { $script:S['backupHours'] = 6; $changed = $true }
  if (-not $script:S.ContainsKey('restartHours')) { $script:S['restartHours'] = 0; $changed = $true }
  if ($changed) { Save-Settings }
}

# ---------- server.properties ----------
function Get-Prop($key, $default) {
  $f = Join-Path $sd 'server.properties'
  if (Test-Path $f) {
    foreach ($l in (Get-Content $f)) { if ($l -like "$key=*") { return $l.Substring($key.Length + 1) } }
  }
  return $default
}
function Set-Prop($key, $value) {
  $f = Join-Path $sd 'server.properties'
  $list = New-Object System.Collections.Generic.List[string]
  $found = $false
  if (Test-Path $f) {
    foreach ($l in (Get-Content $f)) {
      if ($l -like "$key=*") { $list.Add("$key=$value"); $found = $true } else { $list.Add($l) }
    }
  }
  if (-not $found) { $list.Add("$key=$value") }
  Set-Content -Path $f -Value $list.ToArray() -Encoding ASCII
}

# ---------- Java ----------
function Get-JavaExe($jv) {
  $jdir = Join-Path $root "java-$jv"
  $found = Get-ChildItem $jdir -Recurse -Filter java.exe -ErrorAction SilentlyContinue | Where-Object { $_.FullName -match '[\\/]bin[\\/]' } | Select-Object -First 1
  if (-not $found) {
    Say "Getting Java $jv (one-time download)..."
    $arch = if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64') { 'aarch64' } else { 'x64' }
    $zip = Join-Path $root 'java.zip'
    Get-File "https://api.adoptium.net/v3/binary/latest/$jv/ga/windows/$arch/jre/hotspot/normal/eclipse" $zip
    Expand-Archive -Path $zip -DestinationPath $jdir -Force
    Remove-Item $zip
    $found = Get-ChildItem $jdir -Recurse -Filter java.exe | Where-Object { $_.FullName -match '[\\/]bin[\\/]' } | Select-Object -First 1
  }
  return $found.FullName
}

# ---------- is it running? ----------
function Get-ServerProc {
  return (Get-Process -Name java -ErrorAction SilentlyContinue | Where-Object { $_.Path -and $_.Path.StartsWith($root, [StringComparison]::OrdinalIgnoreCase) } | Select-Object -First 1)
}

# ---------- RCON: how the control panel talks to the running server ----------
function Read-Exact($st, $buf, $n) {
  $o = 0
  while ($o -lt $n) {
    $r = $st.Read($buf, $o, $n - $o)
    if ($r -le 0) { throw 'connection closed' }
    $o += $r
  }
}
function New-RconPacket([int]$id, [int]$type, [string]$body) {
  $b = [Text.Encoding]::UTF8.GetBytes($body)
  $ms = New-Object IO.MemoryStream
  $w = New-Object IO.BinaryWriter($ms)
  $w.Write([int](10 + $b.Length))
  $w.Write([int]$id)
  $w.Write([int]$type)
  $w.Write([byte[]]$b)
  $w.Write([byte]0)
  $w.Write([byte]0)
  $w.Flush()
  return , $ms.ToArray()
}
function Read-RconPacket($st) {
  $lb = New-Object byte[] 4
  Read-Exact $st $lb 4
  $len = [BitConverter]::ToInt32($lb, 0)
  if ($len -lt 10 -or $len -gt 8192) { throw 'bad packet' }
  $buf = New-Object byte[] $len
  Read-Exact $st $buf $len
  return @{ id = [BitConverter]::ToInt32($buf, 0); type = [BitConverter]::ToInt32($buf, 4); body = [Text.Encoding]::UTF8.GetString($buf, 8, $len - 10) }
}
# returns @{ ok = $true/$false; text = '...' }
function Rcon([string]$cmd) {
  $c = New-Object System.Net.Sockets.TcpClient
  try {
    $iar = $c.BeginConnect('127.0.0.1', $RconPort, $null, $null)
    if (-not $iar.AsyncWaitHandle.WaitOne(1500)) { throw 'timeout' }
    $c.EndConnect($iar)
    $st = $c.GetStream()
    $st.ReadTimeout = 4000
    $st.WriteTimeout = 4000
    $p = New-RconPacket 1 3 $script:S.rconPassword
    $st.Write($p, 0, $p.Length)
    $r = Read-RconPacket $st
    if ($r.type -eq 2 -and $r.id -eq -1) { throw 'bad password' }
    if ($r.type -ne 2) { $r = Read-RconPacket $st; if ($r.id -eq -1) { throw 'bad password' } }
    $p = New-RconPacket 2 2 $cmd
    $st.Write($p, 0, $p.Length)
    try { $r = Read-RconPacket $st; $text = $r.body } catch { $text = '' }
    $text = $text -replace ([regex]::Escape([string][char]0xA7) + '.'), ''
    return @{ ok = $true; text = $text.Trim() }
  } catch {
    return @{ ok = $false; text = "$($_.Exception.Message)" }
  } finally {
    $c.Close()
  }
}

# ---------- backups ----------
function Zip-Folders($folders, $dest) {
  Add-Type -AssemblyName System.IO.Compression
  $fs = [IO.File]::Open($dest, 'Create')
  $zip = New-Object IO.Compression.ZipArchive($fs, [IO.Compression.ZipArchiveMode]::Create)
  try {
    foreach ($f in $folders) {
      $base = (Split-Path $f -Parent)
      foreach ($file in (Get-ChildItem $f -Recurse -File -ErrorAction SilentlyContinue | Where-Object { $_.Name -ne 'session.lock' })) {
        try {
          $rel = ($file.FullName.Substring($base.Length).TrimStart('\', '/')) -replace '\\', '/'
          $src = [IO.File]::Open($file.FullName, 'Open', 'Read', 'ReadWrite')
          try {
            $entry = $zip.CreateEntry($rel, [IO.Compression.CompressionLevel]::Optimal)
            $es = $entry.Open()
            try { $src.CopyTo($es) } finally { $es.Dispose() }
          } finally { $src.Dispose() }
        } catch { }
      }
    }
  } finally { $zip.Dispose(); $fs.Dispose() }
}
function New-Backup {
  $worlds = @(Get-ChildItem $sd -Directory -Filter 'world*' -ErrorAction SilentlyContinue)
  if ($worlds.Count -eq 0) { return $null }
  New-Item -ItemType Directory -Force -Path $bk | Out-Null
  $zip = Join-Path $bk ('backup-' + (Stamp) + '.zip')
  Zip-Folders ($worlds | ForEach-Object { $_.FullName }) $zip
  Get-ChildItem $bk -Filter *.zip | Sort-Object Name -Descending | Select-Object -Skip 10 | Remove-Item
  return $zip
}
function Restore-Backup($zipPath) {
  $aside = Join-Path $sd ('pre-restore-' + (Stamp))
  New-Item -ItemType Directory -Force -Path $aside | Out-Null
  foreach ($w in @(Get-ChildItem $sd -Directory -Filter 'world*')) { Move-Item -Path $w.FullName -Destination $aside }
  Expand-Archive -Path $zipPath -DestinationPath $sd -Force
  return $aside
}

# ---------- players (whitelist / admins) ----------
function Get-Uuid($n) {
  try {
    $i = (Invoke-RestMethod -Uri "https://api.mojang.com/users/profiles/minecraft/$n" -UserAgent $UA).id
    return "$($i.Substring(0,8))-$($i.Substring(8,4))-$($i.Substring(12,4))-$($i.Substring(16,4))-$($i.Substring(20))"
  } catch { return $null }
}
function Read-List($file) {
  $f = Join-Path $sd $file
  if (-not (Test-Path $f)) { return @() }
  $raw = Get-Content $f -Raw
  if (-not $raw -or -not $raw.Trim()) { return @() }
  return @($raw | ConvertFrom-Json)
}
function Write-List($file, $items) {
  ConvertTo-Json -InputObject @($items) | Set-Content -Path (Join-Path $sd $file) -Encoding ASCII
}
# works whether the server is running (asks the server) or stopped (edits the files)
function Add-Player($n, [bool]$admin) {
  if (-not (Test-Name $n)) { Warn "'$n' is not a valid Minecraft username."; return }
  if (Get-ServerProc) {
    $r = Rcon "whitelist add $n"; if (-not $r.ok) { Warn "Server not ready: $($r.text)"; return }
    Write-Host "  $($r.text)"
    if ($admin) { $r = Rcon "op $n"; Write-Host "  $($r.text)" }
    return
  }
  $u = Get-Uuid $n
  if (-not $u) { Warn "Could not find a Minecraft account called '$n' (check the spelling and your internet)."; return }
  $wl = @(Read-List 'whitelist.json' | Where-Object { $_.name -ne $n })
  $wl += [pscustomobject]@{ uuid = $u; name = $n }
  Write-List 'whitelist.json' $wl
  if ($admin) {
    $ops = @(Read-List 'ops.json' | Where-Object { $_.name -ne $n })
    $ops += [pscustomobject]@{ uuid = $u; name = $n; level = 4; bypassesPlayerLimit = $false }
    Write-List 'ops.json' $ops
  }
  Say "  Added $n$(if ($admin) { ' as an admin' })."
}
function Remove-Player($n, [bool]$adminOnly) {
  if (-not (Test-Name $n)) { Warn "'$n' is not a valid Minecraft username."; return }
  if (Get-ServerProc) {
    if (-not $adminOnly) { $r = Rcon "whitelist remove $n"; Write-Host "  $($r.text)" }
    $r = Rcon "deop $n"; Write-Host "  $($r.text)"
    return
  }
  if (-not $adminOnly) { Write-List 'whitelist.json' @(Read-List 'whitelist.json' | Where-Object { $_.name -ne $n }) }
  Write-List 'ops.json' @(Read-List 'ops.json' | Where-Object { $_.name -ne $n })
  Say "  Done."
}
function Show-Players {
  $ops = @(Read-List 'ops.json' | ForEach-Object { $_.name })
  $wl = @(Read-List 'whitelist.json')
  if ($wl.Count -eq 0) { Write-Host '  Nobody is allowed in yet. Add players below.' }
  foreach ($p in $wl) { Write-Host ("  {0}{1}" -f $p.name, $(if ($ops -contains $p.name) { '   (admin)' } else { '' })) }
}

# ---------- sleep / tunnel ----------
function Prevent-Sleep {
  if ($env:OS -ne 'Windows_NT') { return }
  foreach ($a in @('standby-timeout-ac', 'hibernate-timeout-ac')) { try { powercfg /change $a 0 | Out-Null } catch { } }
  try { powercfg /setacvalueindex SCHEME_CURRENT SUB_BUTTONS LIDACTION 0 | Out-Null; powercfg /setactive SCHEME_CURRENT | Out-Null } catch { }
  try {
    if (-not ('Win.Power' -as [type])) { Add-Type -Namespace Win -Name Power -MemberDefinition '[DllImport("kernel32.dll")] public static extern uint SetThreadExecutionState(uint f);' }
    [void][Win.Power]::SetThreadExecutionState(2147483649)
  } catch { }
}
function Start-Tunnel {
  $pl = Join-Path $root 'playit.exe'
  if (-not (Test-Path $pl)) { Get-File 'https://github.com/playit-cloud/playit-agent/releases/latest/download/playit-windows-x86_64-signed.exe' $pl }
  if (-not (Get-Process playit -ErrorAction SilentlyContinue)) { Start-Process $pl }
  Say 'A second window (playit) opened. First time only: open the link it shows, sign in (free), and add a "Minecraft Java" tunnel to port 25565. The address it gives you is what friends type into Minecraft. Then press I in the control panel and P to save it there.'
}

# ---------- first-time setup ----------
function First-TimeSetup {
  Say 'First-time setup'
  Write-Host 'To run a Minecraft server you must accept the Minecraft EULA: https://aka.ms/MinecraftEULA'
  if ((Read-Host 'Type YES to accept') -ne 'YES') { throw 'EULA not accepted.' }
  $names = Read-Host 'Your Minecraft username (you become an admin; add friends later from the menu)'
  $tunnel = (Read-Host 'Let friends join over the internet? (Y/N)') -match '^[Yy]'
  $heap = $script:HW.heap
  $in = Read-Host ("RAM for Minecraft in GB (this PC has {0} GB, recommended {1} GB). Press Enter for recommended" -f (GB $script:HW.ram), (GB $heap))
  $gb = 0.0
  if ($in -and [double]::TryParse($in, [ref]$gb)) { $heap = [int][math]::Max(1024, [math]::Min($script:HW.ram - 1024, [math]::Floor($gb * 1024))) }

  Say 'Finding the newest Paper (Minecraft server) build...'
  $pick = Find-Paper $null
  if (-not $pick) { throw 'Could not find a stable Paper build.' }
  Say "Minecraft $($pick.v) (build $($pick.b.id))"
  Say 'Downloading the server...'
  $script:S = @{ tunnel = [bool]$tunnel; heapMb = $heap; rconPassword = (New-Password); backupHours = 6; restartHours = 0 }
  Install-Paper $pick

  Set-Content -Path (Join-Path $sd 'eula.txt') -Value 'eula=true' -Encoding ASCII
  Set-Prop 'server-port' '25565'; Set-Prop 'max-players' "$($script:HW.players)"
  Set-Prop 'view-distance' "$($script:HW.view)"; Set-Prop 'simulation-distance' "$($script:HW.sim)"
  Set-Prop 'white-list' 'true'; Set-Prop 'enforce-whitelist' 'true'; Set-Prop 'online-mode' 'true'; Set-Prop 'motd' 'My Minecraft server'
  Write-List 'whitelist.json' @(); Write-List 'ops.json' @()
  $first = $true
  foreach ($n in ($names -split ',' | ForEach-Object { $_.Trim() } | Where-Object { $_ })) { Add-Player $n $first; $first = $false }
  Say 'Setup done.'
}
function Run-Server {
  try { $Host.UI.RawUI.WindowTitle = 'Minecraft Server (running) - manage it from the control panel' } catch { }
  $java = Get-JavaExe $script:S.javaVersion
  Set-Prop 'enable-rcon' 'true'; Set-Prop 'rcon.port' "$RconPort"; Set-Prop 'rcon.password' $script:S.rconPassword; Set-Prop 'broadcast-rcon-to-ops' 'false'
  $prevPlan = Enter-PerfMode
  Prevent-Sleep
  $z = New-Backup
  if ($z) { Write-Host "Backed up your world to $z" }
  if ($script:S.tunnel) { Start-Tunnel }
  if (Test-Path $stopFlag) { Remove-Item $stopFlag -Force }
  $heap = [int]$script:S.heapMb
  $tick = if ($env:BH_TICK) { [int]$env:BH_TICK } else { 5 }
  $warnSec = if ($env:BH_WARN) { [int]$env:BH_WARN } else { 60 }
  $crashes = @(); $gaveUp = $false
  while ($true) {
    Say "Starting the server with $(GB $heap) GB of RAM. To stop it, use the control panel (or type 'stop' here). Do not just close this window."
    $a = @("-Xms$($heap)M", "-Xmx$($heap)M", '-XX:+UseG1GC', '-XX:+ParallelRefProcEnabled', '-XX:MaxGCPauseMillis=200', '-XX:+DisableExplicitGC', '-jar', 'server.jar', '--nogui')
    $p = Start-Process -FilePath $java -ArgumentList $a -WorkingDirectory $sd -NoNewWindow -PassThru
    $null = $p.Handle
    $started = Get-Date; $lastBackup = Get-Date; $scheduled = $false
    while (-not $p.HasExited) {
      Start-Sleep -Seconds $tick
      if ($scheduled -or (Test-Path $stopFlag)) { continue }
      $now = Get-Date
      $bh = [double]$script:S.backupHours; $rh = [double]$script:S.restartHours
      if ($bh -gt 0 -and ($now - $lastBackup).TotalHours -ge $bh) {
        $zz = Backup-Live
        if ($zz) { Write-Host "Automatic backup saved: $zz" }
        $lastBackup = Get-Date
      }
      if ($rh -gt 0 -and ($now - $started).TotalHours -ge $rh) {
        Warn "Scheduled restart in $warnSec seconds..."
        [void](Rcon "say Scheduled restart in $warnSec seconds!")
        Start-Sleep -Seconds $warnSec
        [void](Rcon 'save-all'); [void](Rcon 'stop')
        $scheduled = $true
      }
    }
    $p.WaitForExit()
    if (Test-Path $stopFlag) { Remove-Item $stopFlag -Force; break }
    if ($scheduled) { Start-Sleep -Seconds 3; continue }
    if ($p.ExitCode -eq 0) { break }
    $now = Get-Date
    $crashes = @($crashes | Where-Object { ($now - $_).TotalMinutes -lt 10 }) + $now
    if ($crashes.Count -gt 5) { Bad 'The server keeps crashing, so I stopped trying.'; $gaveUp = $true; break }
    Warn "The server crashed (code $($p.ExitCode)). Restarting in 10 seconds..."
    Start-Sleep -Seconds $(if ($env:BH_TICK) { 1 } else { 10 })
  }
  Exit-PerfMode $prevPlan
  Get-Process playit -ErrorAction SilentlyContinue | Stop-Process -Force
  Say 'Server stopped.'
  if ($gaveUp) { [void](Read-Host 'Press Enter to close') }
}
function Get-LanIp {
  try {
    $ip = [System.Net.Dns]::GetHostAddresses([System.Net.Dns]::GetHostName()) | Where-Object { $_.AddressFamily -eq 'InterNetwork' -and $_.ToString() -notlike '169.254.*' -and $_.ToString() -notlike '127.*' } | Select-Object -First 1
    if ($ip) { return $ip.ToString() }
  } catch { }
  return $null
}
function Show-Header {
  Clear-Host
  $proc = Get-ServerProc
  $status = 'STOPPED'; $online = $null
  if ($proc) {
    $r = Rcon 'list'
    if ($r.ok) { $status = 'RUNNING'; $online = $r.text } else { $status = 'STARTING (or not reachable yet)' }
  }
  Write-Host '=========== Minecraft Server Control Panel ===========' -ForegroundColor Cyan
  $build = if ($script:S.ContainsKey('paperBuild')) { " (Paper build $($script:S.paperBuild))" } else { '' }
  Write-Host ("Minecraft version : {0}{1}" -f $script:S.minecraft, $build)
  Write-Host ("                    Friends must use Minecraft {0} to join." -f $script:S.minecraft) -ForegroundColor DarkGray
  Write-Host ("Status            : {0}" -f $status) -ForegroundColor $(if ($status -eq 'RUNNING') { 'Green' } elseif ($status -eq 'STOPPED') { 'Red' } else { 'Yellow' })
  if ($online) { Write-Host ("Players           : {0}" -f $online) }
  Write-Host ("RAM for server    : {0} GB   (this PC: {1} GB, recommended: {2} GB)" -f (GB $script:S.heapMb), (GB $script:HW.ram), (GB $script:HW.heap))
  Write-Host ("Max players       : {0}" -f (Get-Prop 'max-players' '?'))
  Write-Host  'Join from this PC : localhost'
  $lan = Get-LanIp
  if ($lan) { Write-Host ("Join on same WiFi : {0}" -f $lan) }
  if ($script:S.playitAddress) { Write-Host ('Join from internet: {0}' -f $script:S.playitAddress) -ForegroundColor Green } elseif ($script:S.tunnel) { Write-Host 'Join from internet: on - press I, then P to save your playit address' } else { Write-Host 'Join from internet: off (turn on in Settings)' }
  if ($script:UpdateHint) { Write-Host $script:UpdateHint -ForegroundColor Yellow }
  Write-Host '======================================================' -ForegroundColor Cyan
}
function Wait-ForStart {
  Write-Host 'Starting (the first start builds the world and can take a minute or two)...' -NoNewline
  for ($i = 0; $i -lt 60; $i++) {
    Start-Sleep -Seconds 2
    Write-Host '.' -NoNewline
    if ((Rcon 'list').ok) { Write-Host ''; Say 'Server is up!'; return }
    if (-not (Get-ServerProc) -and $i -gt 10) { break }
  }
  Write-Host ''
  Warn 'Not ready yet. Check the server window for messages.'
}
function Start-ServerWindow {
  if (Get-ServerProc) { Warn 'The server is already running.'; return }
  if (Test-Path $stopFlag) { Remove-Item $stopFlag -Force }
  if (-not $env:BH_BAT) { Bad 'Cannot find the launcher file. Start the server by running the .bat with the word "server" after it.'; return }
  Start-Process -FilePath $env:BH_BAT -ArgumentList 'server'
  Wait-ForStart
}
function Stop-ServerSafely([int]$warnSeconds) {
  if (-not (Get-ServerProc)) { Warn 'The server is not running.'; return $true }
  New-Item -ItemType File -Force -Path $stopFlag | Out-Null
  if ($warnSeconds -gt 0) {
    $r = Rcon "say Server stopping in $warnSeconds seconds!"
    if (-not $r.ok) { Remove-Item $stopFlag -Force -ErrorAction SilentlyContinue; Warn "Could not reach the server yet ($($r.text)). Try again in a moment."; return $false }
    Write-Host "Warned players. Waiting $warnSeconds seconds..."
    Start-Sleep -Seconds $warnSeconds
  }
  [void](Rcon 'save-all')
  $r = Rcon 'stop'
  if (-not $r.ok) { Remove-Item $stopFlag -Force -ErrorAction SilentlyContinue; Warn "Could not reach the server ($($r.text)). It may still be starting. Try again, or use Force stop."; return $false }
  Write-Host 'Saving and stopping...' -NoNewline
  for ($i = 0; $i -lt 45; $i++) {
    if (-not (Get-ServerProc)) { Write-Host ''; Say 'Server stopped safely.'; return $true }
    Start-Sleep -Seconds 2; Write-Host '.' -NoNewline
  }
  Write-Host ''
  Warn 'It is taking a long time. Wait a bit more, or use Force stop.'
  return $false
}
function Force-Stop {
  $p = Get-ServerProc
  if (-not $p) { Warn 'The server is not running.'; return }
  Warn 'Force stop cuts the server off without saving. You can lose the last few minutes of building.'
  if ((Read-Host 'Type YES to force stop') -ne 'YES') { return }
  New-Item -ItemType File -Force -Path $stopFlag | Out-Null
  Stop-Process -Id $p.Id -Force
  Say 'Stopped.'
}
function Offer-Restart {
  if (Get-ServerProc) {
    if ((Read-Host 'The server is running. Restart it now to apply this? (Y/N)') -match '^[Yy]') {
      if (Stop-ServerSafely 0) { Start-Sleep -Seconds 3; Start-ServerWindow }
    } else { Warn 'OK, it will apply the next time the server starts.' }
  }
}

function Get-PublicIp {
  try { return (Invoke-RestMethod -Uri 'https://api.ipify.org' -UserAgent $UA -TimeoutSec 5).ToString().Trim() } catch { return $null }
}
function Get-DirSizeMb($paths) {
  $sum = 0
  foreach ($p in $paths) {
    $m = Get-ChildItem $p -Recurse -File -ErrorAction SilentlyContinue | Measure-Object -Property Length -Sum
    if ($m.Sum) { $sum += $m.Sum }
  }
  return [math]::Round($sum / 1MB, 0)
}
function Get-FreeDiskGb {
  try { $d = New-Object IO.DriveInfo([IO.Path]::GetPathRoot($root)); return [math]::Round($d.AvailableFreeSpace / 1GB, 1) } catch { return $null }
}
function Format-Uptime($ts) { return ('{0}d {1}h {2}m' -f [int][math]::Floor($ts.TotalDays), $ts.Hours, $ts.Minutes) }
function Test-Address($a) { return ($a -match '^[A-Za-z0-9.\-]+(:\d{1,5})?$') }
function Show-Log {
  Clear-Host
  $f = Join-Path $sd 'logs/latest.log'
  if (-not (Test-Path $f)) { Warn 'No log yet. Start the server once first.'; Wait-Enter; return }
  Write-Host '--- Last 40 lines of the server log ---' -ForegroundColor Cyan
  foreach ($l in (Get-Content $f -Tail 40)) { Write-Host $l }
  Write-Host ''
  Wait-Enter
}
function Menu-Info {
  $port = Get-Prop 'server-port' '25565'
  $sfx = if ($port -ne '25565') { ":$port" } else { '' }
  Write-Host 'Checking your public IP...'
  $pubIp = Get-PublicIp
  while ($true) {
    Clear-Host
    Write-Host '--- Server info and addresses ---' -ForegroundColor Cyan
    $proc = Get-ServerProc
    Write-Host ("Minecraft version : {0}   (friends need this exact version)" -f $script:S.minecraft)
    if ($proc) {
      $r = Rcon 'list'
      Write-Host ("Status            : {0}" -f $(if ($r.ok) { 'RUNNING' } else { 'STARTING' })) -ForegroundColor Green
      try { Write-Host ("Uptime            : {0}" -f (Format-Uptime ((Get-Date) - $proc.StartTime))) } catch { }
      Write-Host ("Memory in use     : {0} GB of {1} GB allowed" -f (GB ($proc.WorkingSet64 / 1MB)), (GB $script:S.heapMb))
      if ($r.ok) {
        Write-Host ("Players           : {0}" -f $r.text)
        $t = Rcon 'tps'
        if ($t.ok -and $t.text) { Write-Host ("Speed (TPS)       : {0}   (20 is perfect; lower means lag)" -f $t.text) }
      }
    } else {
      Write-Host 'Status            : STOPPED' -ForegroundColor Red
    }
    $worlds = @(Get-ChildItem $sd -Directory -Filter 'world*' -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName })
    Write-Host ("World size        : {0} MB" -f (Get-DirSizeMb $worlds))
    $free = Get-FreeDiskGb
    if ($null -ne $free) { Write-Host ("Free disk space   : {0} GB" -f $free) }
    Write-Host ''
    Write-Host 'Addresses - type one into Minecraft > Multiplayer > Add Server:' -ForegroundColor Cyan
    Write-Host ("  This PC              : localhost{0}" -f $sfx)
    $lan = Get-LanIp
    if ($lan) { Write-Host ("  Same WiFi / home     : {0}{1}" -f $lan, $sfx) }
    if ($script:S.playitAddress) { Write-Host ("  Over the internet    : {0}   <- share this with friends" -f $script:S.playitAddress) -ForegroundColor Green }
    elseif ($script:S.tunnel) { Write-Host '  Over the internet    : not saved yet - look in the playit window, then press P here' -ForegroundColor Yellow }
    else { Write-Host '  Over the internet    : off (turn it on in Settings)' }
    if ($pubIp) { Write-Host ("  Your public IP       : {0}{1}   (only works if you forward port {2} on your router; many internet providers block this)" -f $pubIp, $sfx, $port) -ForegroundColor DarkGray }
    Write-Host ''
    Write-Host ' C) Copy the best address to share   P) Save/change my playit address   D) Open playit dashboard'
    Write-Host ' L) Show the server log              R) Refresh                          B) Back'
    $c = (Read-Host 'Choose').Trim().ToLower()
    if ($c -eq 'b' -or $c -eq '') { return }
    switch ($c) {
      'c' {
        $best = if ($script:S.playitAddress) { $script:S.playitAddress } elseif ($lan) { "$lan$sfx" } else { "localhost$sfx" }
        try { Set-Clipboard -Value $best; Say "Copied: $best" } catch { Warn "Could not copy. The address is: $best" }
        Wait-Enter
      }
      'p' {
        $a = (Read-Host 'Paste the address from your playit tunnel, e.g. something.joinmc.link (Enter to cancel)').Trim()
        if ($a -and (Test-Address $a)) { $script:S['playitAddress'] = $a; Save-Settings; Say 'Saved. It now shows on the main screen too.' }
        elseif ($a) { Warn 'That does not look like an address (letters, numbers, dots and dashes only).' }
        Wait-Enter
      }
      'd' { if ($env:OS -eq 'Windows_NT') { Start-Process 'https://playit.gg/account/tunnels' } else { Write-Host 'https://playit.gg/account/tunnels' } }
      'l' { Show-Log }
      'r' { $pubIp = Get-PublicIp }
    }
  }
}

function Menu-Players {
  while ($true) {
    Clear-Host
    Write-Host '--- Players ---' -ForegroundColor Cyan
    Show-Players
    if (Get-ServerProc) { $r = Rcon 'list'; if ($r.ok) { Write-Host ''; Write-Host "  Online now: $($r.text)" } }
    Write-Host ''
    Write-Host ' 1) Allow a player in          4) Remove a player'
    Write-Host ' 2) Allow a player + make admin 5) Remove admin rights'
    Write-Host ' 3) Make an existing player admin'
    Write-Host ' B) Back'
    $c = (Read-Host 'Choose').Trim().ToLower()
    if ($c -eq 'b' -or $c -eq '') { return }
    if ($c -notin @('1', '2', '3', '4', '5')) { continue }
    $n = (Read-Host 'Minecraft username').Trim()
    switch ($c) {
      '1' { Add-Player $n $false }
      '2' { Add-Player $n $true }
      '3' { Add-Player $n $true }
      '4' { Remove-Player $n $false }
      '5' { Remove-Player $n $true }
    }
    Wait-Enter
  }
}
function Menu-Actions {
  if (-not (Get-ServerProc)) { Warn 'Start the server first.'; Wait-Enter; return }
  while ($true) {
    Clear-Host
    Write-Host '--- Admin actions (server must be running) ---' -ForegroundColor Cyan
    Write-Host ' 1) Send a message to everyone    5) Set time to day'
    Write-Host ' 2) Kick a player                 6) Clear the weather'
    Write-Host ' 3) Ban a player                  7) Save the world now'
    Write-Host ' 4) Unban a player                8) Type any server command'
    Write-Host ' B) Back'
    $c = (Read-Host 'Choose').Trim().ToLower()
    if ($c -eq 'b' -or $c -eq '') { return }
    $cmd = $null
    switch ($c) {
      '1' { $t = Read-Host 'Message'; if ($t) { $cmd = "say $t" } }
      '2' { $n = (Read-Host 'Player to kick').Trim(); if (Test-Name $n) { $cmd = "kick $n" } else { Warn 'Invalid username.' } }
      '3' { $n = (Read-Host 'Player to ban').Trim(); if (Test-Name $n) { $cmd = "ban $n" } else { Warn 'Invalid username.' } }
      '4' { $n = (Read-Host 'Player to unban').Trim(); if (Test-Name $n) { $cmd = "pardon $n" } else { Warn 'Invalid username.' } }
      '5' { $cmd = 'time set day' }
      '6' { $cmd = 'weather clear' }
      '7' { $cmd = 'save-all' }
      '8' { $t = (Read-Host 'Command (without the /)').Trim().TrimStart('/'); if ($t) { $cmd = $t } }
    }
    if ($cmd) {
      $r = Rcon $cmd
      if ($r.ok) { if ($r.text) { Write-Host "  $($r.text)" } else { Say '  Done.' } } else { Warn "  Could not reach the server: $($r.text)" }
    }
    Wait-Enter
  }
}
function Menu-Settings {
  while ($true) {
    Clear-Host
    Write-Host '--- Settings ---' -ForegroundColor Cyan
    $wlOn = ((Get-Prop 'white-list' 'false') -eq 'true')
    $rh = [double]$script:S.restartHours; $bh = [double]$script:S.backupHours
    Write-Host (" 1) RAM for Minecraft    : {0} GB   (this PC {1} GB, recommended {2} GB)" -f (GB $script:S.heapMb), (GB $script:HW.ram), (GB $script:HW.heap))
    Write-Host (" 2) Max players          : {0}" -f (Get-Prop 'max-players' '?'))
    Write-Host (" 3) View distance        : {0} chunks (lower = faster)" -f (Get-Prop 'view-distance' '?'))
    Write-Host (" 4) Internet access      : {0}" -f $(if ($script:S.tunnel) { 'on (playit.gg)' } else { 'off' }))
    Write-Host (" 5) Server name (MOTD)   : {0}" -f (Get-Prop 'motd' ''))
    Write-Host (" 6) Auto-restart         : {0}" -f $(if ($rh -gt 0) { "every $rh hours" } else { 'off' }))
    Write-Host (" 7) Auto-backup          : {0}" -f $(if ($bh -gt 0) { "every $bh hours while running" } else { 'off (still backs up on every start)' }))
    Write-Host (" 8) Start with Windows   : {0}" -f $(if (Test-AutoStart) { 'on (starts when you log in)' } else { 'off' }))
    Write-Host (" 9) Only allowed players : {0}" -f $(if ($wlOn) { 'on (whitelist, recommended)' } else { 'OFF - anyone with the address can join' }))
    Write-Host ' L) Low-lag mode for old laptops (short view distance)   R) Reset speed settings to recommended'
    Write-Host ' S) Put a shortcut on my desktop        B) Back'
    $c = (Read-Host 'Choose').Trim().ToLower()
    if ($c -eq 'b' -or $c -eq '') { return }
    $changed = $false
    switch ($c) {
      '1' {
        $in = Read-Host 'New RAM in GB, e.g. 4 or 2.5 (Enter to cancel)'
        $gb = 0.0
        if ($in -and [double]::TryParse($in, [ref]$gb)) {
          $mb = [int][math]::Floor($gb * 1024)
          $maxMb = $script:HW.ram - 1024
          if ($mb -lt 1024) { Warn 'Minecraft needs at least 1 GB. Using 1 GB.'; $mb = 1024 }
          if ($mb -gt $maxMb) { Warn "That leaves too little for Windows. Using $(GB $maxMb) GB."; $mb = $maxMb }
          if ($mb -gt ($script:HW.heap * 1.3)) { Warn 'That is more than recommended and can make this PC sluggish.' }
          $script:S['heapMb'] = $mb; Save-Settings; $changed = $true; Say "RAM set to $(GB $mb) GB."
        }
      }
      '2' { $n = Read-Int 'Max players (1-100, Enter to cancel)' 1 100; if ($null -ne $n) { Set-Prop 'max-players' "$n"; $changed = $true; Say "Max players set to $n." } }
      '3' { $n = Read-Int 'View distance in chunks (3-32, Enter to cancel)' 3 32; if ($null -ne $n) { Set-Prop 'view-distance' "$n"; $changed = $true; Say "View distance set to $n." } }
      '4' { $script:S['tunnel'] = -not [bool]$script:S.tunnel; Save-Settings; $changed = $true; Say "Internet access is now $(if ($script:S.tunnel) { 'on' } else { 'off' })." }
      '5' {
        $in = (Read-Host 'Server name shown in the Minecraft server list (letters/numbers/spaces)').Trim()
        if ($in -match '^[A-Za-z0-9 _.!-]{1,40}$') { Set-Prop 'motd' $in; $changed = $true; Say 'Name updated.' } elseif ($in) { Warn 'Use plain letters, numbers and spaces.' }
      }
      '6' { $n = Read-Int 'Restart the server every how many hours? (0 = never, max 168)' 0 168; if ($null -ne $n) { $script:S['restartHours'] = $n; Save-Settings; $changed = $true; Say $(if ($n -gt 0) { "It will restart itself every $n hours, with a 60-second warning." } else { 'Auto-restart is off.' }) } }
      '7' { $n = Read-Int 'Back up the world every how many hours while running? (0 = off, max 168)' 0 168; if ($null -ne $n) { $script:S['backupHours'] = $n; Save-Settings; $changed = $true; Say $(if ($n -gt 0) { "Backing up every $n hours." } else { 'Timed backups are off.' }) } }
      '8' {
        if (Test-AutoStart) { Set-AutoStart $false; Say 'The server will no longer start by itself.' }
        else { Set-AutoStart $true; Say 'The server will start by itself when you log in to Windows. For a PC nobody sits at, also set Windows to log in automatically (search "netplwiz").'; Warn 'If you move this file, turn this off and on again.' }
        Wait-Enter
      }
      '9' {
        if ($wlOn) {
          Warn 'Turning this off lets ANYONE who finds your address join, build, and grief.'
          if ((Read-Host 'Type YES to turn it off') -eq 'YES') { Set-Prop 'white-list' 'false'; Set-Prop 'enforce-whitelist' 'false'; if (Get-ServerProc) { [void](Rcon 'whitelist off') } }
        } else { Set-Prop 'white-list' 'true'; Set-Prop 'enforce-whitelist' 'true'; if (Get-ServerProc) { [void](Rcon 'whitelist on') }; Say 'Only allowed players can join again.' }
        Wait-Enter
      }
      's' { Make-Shortcut; Wait-Enter }
      'l' { Apply-LowLag; $changed = $true; Say 'Low-lag mode on: view distance 6, simulation distance 4, faster chunk saving.' }
      'r' { Reset-Recommended; $changed = $true; Say 'Speed settings reset to what suits this PC.' }
    }
    if ($changed) { Offer-Restart; Wait-Enter }
  }
}
function Menu-Backups {
  while ($true) {
    Clear-Host
    Write-Host '--- Backups ---' -ForegroundColor Cyan
    $list = @(Get-ChildItem $bk -Filter *.zip -ErrorAction SilentlyContinue | Sort-Object Name -Descending)
    if ($list.Count -eq 0) { Write-Host '  No backups yet. One is made automatically each time the server starts.' }
    for ($i = 0; $i -lt $list.Count; $i++) { Write-Host ("  {0}) {1}   ({2} MB)" -f ($i + 1), $list[$i].Name, [math]::Round($list[$i].Length / 1MB, 1)) }
    Write-Host ''
    Write-Host ' N) Back up now      R) Restore one of the above      O) Open backups folder      B) Back'
    $c = (Read-Host 'Choose').Trim().ToLower()
    if ($c -eq 'b' -or $c -eq '') { return }
    if ($c -eq 'o') { if ($env:OS -eq 'Windows_NT') { New-Item -ItemType Directory -Force -Path $bk | Out-Null; Start-Process explorer.exe $bk } }
    if ($c -eq 'n') {
      $z = Backup-Live
      if ($z) { Say "Saved $z" } else { Warn 'No backup made (the world does not exist yet, or the server is still starting).' }
      Wait-Enter
    }
    if ($c -eq 'r') {
      if (Get-ServerProc) { Warn 'Stop the server first, then restore.'; Wait-Enter; continue }
      $n = 0
      if ([int]::TryParse((Read-Host 'Number of the backup to restore'), [ref]$n) -and $n -ge 1 -and $n -le $list.Count) {
        if ((Read-Host 'This replaces the current world (the old one is kept aside). Type YES') -eq 'YES') {
          $aside = Restore-Backup $list[$n - 1].FullName
          Say "Restored. Your previous world was kept in $aside"
        }
        Wait-Enter
      }
    }
  }
}

# ---------- more helpers ----------
function Read-Int($prompt, $min, $max) {
  $in = Read-Host $prompt; $n = 0
  if ($in -and [int]::TryParse($in, [ref]$n) -and $n -ge $min -and $n -le $max) { return $n }
  return $null
}
function Pick-Option($title, $opts) {
  Write-Host $title
  for ($i = 0; $i -lt $opts.Count; $i++) { Write-Host ("  {0}) {1}" -f ($i + 1), $opts[$i]) }
  $n = Read-Int 'Choose (Enter to cancel)' 1 $opts.Count
  if ($null -eq $n) { return $null }
  return $opts[$n - 1]
}
function Backup-Live {
  $live = [bool](Get-ServerProc)
  if ($live) {
    $r = Rcon 'save-off'
    if (-not $r.ok) { return $null }
    [void](Rcon 'save-all flush')
    Start-Sleep -Seconds $(if ($env:BH_TICK) { 1 } else { 8 })
  }
  $z = $null
  try { $z = New-Backup } catch { Bad "Backup failed: $($_.Exception.Message)" } finally { if ($live) { [void](Rcon 'save-on') } }
  return $z
}
function Get-StartupFile {
  $d = if ($env:BH_STARTUP) { $env:BH_STARTUP } else { [Environment]::GetFolderPath('Startup') }
  return (Join-Path $d 'MinecraftServer.cmd')
}
function Test-AutoStart { return (Test-Path (Get-StartupFile)) }
function Set-AutoStart([bool]$on) {
  $f = Get-StartupFile
  if ($on) {
    if (-not $env:BH_BAT) { Warn 'Cannot find the launcher file path.'; return }
    Set-Content -Path $f -Value ("@echo off`r`nstart `"`" `"$($env:BH_BAT)`" server") -Encoding ASCII
  } elseif (Test-Path $f) { Remove-Item $f -Force }
}
function Make-Shortcut {
  if ($env:OS -ne 'Windows_NT' -or -not $env:BH_BAT) { Warn 'Shortcuts only work on Windows.'; return }
  try {
    $ws = New-Object -ComObject WScript.Shell
    $lnk = $ws.CreateShortcut((Join-Path ([Environment]::GetFolderPath('Desktop')) 'Minecraft Server.lnk'))
    $lnk.TargetPath = $env:BH_BAT
    $lnk.WorkingDirectory = (Split-Path $env:BH_BAT -Parent)
    $lnk.Save()
    Say 'Shortcut added to your desktop.'
  } catch { Warn "Could not create the shortcut: $($_.Exception.Message)" }
}

# ---------- updates ----------
function Find-Paper([string]$want) {
  if ($want) { $vers = @($want) }
  else {
    $proj = Get-Json 'https://fill.papermc.io/v3/projects/paper'
    $vers = @($proj.versions.PSObject.Properties | ForEach-Object { $_.Value }) | Where-Object { $_ -notmatch '-' } | Sort-Object { [version]$_ } -Descending | Select-Object -First 8
  }
  foreach ($v in $vers) {
    $builds = Get-Json "https://fill.papermc.io/v3/projects/paper/versions/$v/builds"
    $stable = @($builds | Where-Object { $_.channel -eq 'STABLE' }) | Sort-Object id -Descending | Select-Object -First 1
    if ($stable) {
      $jv = if ([version]$v -ge [version]'26.0') { 25 } else { 21 }
      try { $m = (Get-Json "https://fill.papermc.io/v3/projects/paper/versions/$v").version.java.version.minimum; if ($m) { $jv = [int]$m } } catch { }
      return @{ v = $v; b = $stable; jv = $jv }
    }
  }
  return $null
}
function Install-Paper($pick) {
  $dl = $pick.b.downloads.'server:default'
  $new = Join-Path $sd 'server.jar.new'
  Get-File $dl.url $new
  if ((Get-FileHash $new -Algorithm SHA256).Hash -ne $dl.checksums.sha256) { Remove-Item $new; throw 'The download was corrupted. Try again.' }
  [void](Get-JavaExe $pick.jv)
  $jar = Join-Path $sd 'server.jar'
  if (Test-Path $jar) { Copy-Item $jar (Join-Path $sd 'server.jar.old') -Force }
  Move-Item $new $jar -Force
  $script:S['minecraft'] = $pick.v; $script:S['paperBuild'] = $pick.b.id; $script:S['javaVersion'] = $pick.jv
  Save-Settings
}
function Check-UpdateHint {
  $script:UpdateHint = $null
  $old = $script:JsonTimeout; $script:JsonTimeout = 6
  try {
    $curBuild = if ($script:S.ContainsKey('paperBuild')) { [int]$script:S.paperBuild } else { 0 }
    $same = Find-Paper $script:S.minecraft
    if ($same -and $same.b.id -gt $curBuild) { $script:UpdateHint = 'Update available for your server (press U).'; return }
    $latest = Find-Paper $null
    if ($latest -and [version]$latest.v -gt [version]$script:S.minecraft) { $script:UpdateHint = "New Minecraft version $($latest.v) is available (press U)." }
  } catch { } finally { $script:JsonTimeout = $old }
}
function Menu-Update {
  if (Get-ServerProc) { Warn 'Stop the server first - updating replaces its files.'; Wait-Enter; return }
  Write-Host 'Checking for updates...'
  try { $same = Find-Paper $script:S.minecraft; $latest = Find-Paper $null }
  catch { Warn "Could not check for updates: $($_.Exception.Message)"; Wait-Enter; return }
  $curBuild = if ($script:S.ContainsKey('paperBuild')) { [int]$script:S.paperBuild } else { 0 }
  Write-Host ("You have: Minecraft {0}, Paper build {1}" -f $script:S.minecraft, $curBuild)
  $canBuild = ($same -and $same.b.id -gt $curBuild)
  $canMajor = ($latest -and [version]$latest.v -gt [version]$script:S.minecraft)
  if (-not $canBuild -and -not $canMajor) { Say 'You are up to date.'; $script:UpdateHint = $null; Wait-Enter; return }
  if ($canBuild) { Write-Host (" 1) Update to build {0} of Minecraft {1}  (safe: bug and security fixes, friends change nothing)" -f $same.b.id, $same.v) }
  if ($canMajor) { Write-Host (" 2) Upgrade to Minecraft {0}  (friends must update their game; worlds cannot go back; some plugins may break)" -f $latest.v) }
  Write-Host ' B) Back'
  $c = (Read-Host 'Choose').Trim().ToLower()
  $pick = $null
  if ($c -eq '1' -and $canBuild) { $pick = $same }
  elseif ($c -eq '2' -and $canMajor) {
    Warn 'Upgrading to a new Minecraft version changes your world permanently. A backup is made first so you can go back.'
    if ((Read-Host 'Type YES to continue') -eq 'YES') { $pick = $latest }
  }
  if (-not $pick) { return }
  try {
    $z = New-Backup
    if ($z) { Write-Host "Backed up your world to $z" }
    Say 'Downloading...'
    Install-Paper $pick
    $script:UpdateHint = $null
    Say "Updated to Minecraft $($pick.v), build $($pick.b.id). Start the server when you are ready."
  } catch { Bad "Update failed: $($_.Exception.Message)" }
  Wait-Enter
}

# ---------- game rules and world ----------
function New-World {
  if (Get-ServerProc) { Warn 'Stop the server first.'; return $false }
  Warn 'This starts a fresh world. Your current world is backed up and kept in the old-worlds folder, so nothing is deleted.'
  if ((Read-Host 'Type YES to continue') -ne 'YES') { return $false }
  $seed = (Read-Host 'World seed (Enter for a random world)').Trim()
  if ($seed -and $seed -notmatch '^[A-Za-z0-9 _-]{1,40}$') { Warn 'A seed can only use letters, numbers, spaces, - and _.'; return $false }
  try { [void](New-Backup) } catch { Bad "Backup failed, so nothing was changed: $($_.Exception.Message)"; return $false }
  $dest = Join-Path (Join-Path $root 'old-worlds') ('world-' + (Stamp))
  New-Item -ItemType Directory -Force -Path $dest | Out-Null
  foreach ($w in @(Get-ChildItem $sd -Directory -Filter 'world*')) { Move-Item -Path $w.FullName -Destination $dest }
  Set-Prop 'level-seed' $seed
  Say "Done. The old world is in $dest. A new world is made the next time you start the server."
  return $true
}
function Menu-Game {
  while ($true) {
    Clear-Host
    Write-Host '--- Game rules and world ---' -ForegroundColor Cyan
    Write-Host (" 1) Difficulty              : {0}" -f (Get-Prop 'difficulty' 'easy'))
    Write-Host (" 2) Default game mode       : {0}  (for players joining the first time)" -f (Get-Prop 'gamemode' 'survival'))
    Write-Host (" 3) Players can fight (PvP) : {0}" -f (Get-Prop 'pvp' 'true'))
    Write-Host (" 4) Command blocks         : {0}" -f (Get-Prop 'enable-command-block' 'false'))
    Write-Host  ' 5) Start a brand-new world (the old one is kept)'
    Write-Host  ' B) Back'
    $c = (Read-Host 'Choose').Trim().ToLower()
    if ($c -eq 'b' -or $c -eq '') { return }
    $changed = $false
    switch ($c) {
      '1' { $v = Pick-Option 'Difficulty:' @('peaceful', 'easy', 'normal', 'hard'); if ($v) { Set-Prop 'difficulty' $v; if (Get-ServerProc) { [void](Rcon "difficulty $v"); Say "Difficulty is now $v (applied right away)." } else { Say "Difficulty set to $v." } } }
      '2' { $v = Pick-Option 'Default game mode:' @('survival', 'creative', 'adventure', 'spectator'); if ($v) { Set-Prop 'gamemode' $v; $changed = $true; Say "Default game mode set to $v." } }
      '3' { $v = Pick-Option 'Players can fight each other:' @('true', 'false'); if ($v) { Set-Prop 'pvp' $v; $changed = $true; Say "PvP set to $v." } }
      '4' { $v = Pick-Option 'Command blocks:' @('true', 'false'); if ($v) { Set-Prop 'enable-command-block' $v; $changed = $true; Say "Command blocks set to $v." } }
      '5' { if (New-World) { } }
    }
    if ($changed) { Offer-Restart }
    Wait-Enter
  }
}

# ---------- plugins (from Modrinth) ----------
function Install-PluginFlow {
  $q = (Read-Host 'What are you looking for? e.g. worldedit, chunky, essentials (Enter to cancel)').Trim()
  if (-not $q) { return }
  $ver = $script:S.minecraft
  $facets = '[["categories:paper"],["versions:' + $ver + '"]]'
  try { $res = Invoke-RestMethod -Uri ("$ModrinthBase/search?query=" + [uri]::EscapeDataString($q) + '&limit=6&facets=' + [uri]::EscapeDataString($facets)) -UserAgent $UA -TimeoutSec 20 }
  catch { Warn "Search failed: $($_.Exception.Message)"; return }
  $hits = @($res.hits)
  if ($hits.Count -eq 0) { Warn "Nothing found for '$q' that works on Minecraft $ver."; return }
  for ($i = 0; $i -lt $hits.Count; $i++) {
    $d = "$($hits[$i].description)"; if ($d.Length -gt 90) { $d = $d.Substring(0, 90) + '...' }
    Write-Host ("  {0}) {1}  ({2} downloads)" -f ($i + 1), $hits[$i].title, $hits[$i].downloads)
    Write-Host ("       {0}" -f $d) -ForegroundColor DarkGray
  }
  $n = Read-Int 'Install which one? (Enter to cancel)' 1 $hits.Count
  if ($null -eq $n) { return }
  $h = $hits[$n - 1]
  try {
    $vu = "$ModrinthBase/project/$($h.project_id)/version?loaders=" + [uri]::EscapeDataString('["paper"]') + '&game_versions=' + [uri]::EscapeDataString('["' + $ver + '"]')
    $vs = @(Invoke-RestMethod -Uri $vu -UserAgent $UA -TimeoutSec 20)
  } catch { Warn "Could not get the download: $($_.Exception.Message)"; return }
  if ($vs.Count -eq 0) { Warn "There is no download of $($h.title) for Minecraft $ver."; return }
  $v = $vs[0]
  $file = @($v.files | Where-Object { $_.primary }) | Select-Object -First 1
  if (-not $file) { $file = @($v.files) | Select-Object -First 1 }
  $name = [IO.Path]::GetFileName("$($file.filename)")
  $urlOk = ("$($file.url)".StartsWith('https://cdn.modrinth.com/')) -or ($env:BH_MODRINTH -and "$($file.url)".StartsWith($env:BH_MODRINTH))
  if (-not $file -or $name -notmatch '^[A-Za-z0-9_.+\- ]+\.jar$' -or -not $urlOk) { Bad 'That download did not look safe, so I skipped it.'; return }
  $deps = @($v.dependencies | Where-Object { $_.dependency_type -eq 'required' })
  if ($deps.Count -gt 0) { Warn "Note: this plugin needs $($deps.Count) other plugin(s) to work. Search for them too if it does not load." }
  if ((Read-Host ("Install {0} {1} by {2}? Plugins are programs - only install ones you trust. (Y/N)" -f $h.title, $v.version_number, $h.author)) -notmatch '^[Yy]') { return }
  $pd = Join-Path $sd 'plugins'
  New-Item -ItemType Directory -Force -Path $pd | Out-Null
  $dest = Join-Path $pd $name
  try {
    Get-File $file.url $dest
    $want = "$($file.hashes.sha512)"
    if ($want -and (Get-FileHash $dest -Algorithm SHA512).Hash -ne $want) { Remove-Item $dest -Force; throw 'The download was corrupted.' }
  } catch { Bad "Install failed: $($_.Exception.Message)"; return }
  Say "Installed $name. Restart the server to load it."
  Offer-Restart
}
function Menu-Plugins {
  $pd = Join-Path $sd 'plugins'
  while ($true) {
    Clear-Host
    Write-Host '--- Plugins ---' -ForegroundColor Cyan
    $jars = @(Get-ChildItem $pd -Filter *.jar -ErrorAction SilentlyContinue)
    if ($jars.Count -eq 0) { Write-Host '  No plugins installed.' } else { for ($i = 0; $i -lt $jars.Count; $i++) { Write-Host ("  {0}) {1}" -f ($i + 1), $jars[$i].Name) } }
    Write-Host ''
    Write-Host ' F) Find and install a plugin (from Modrinth)    R) Remove a plugin    B) Back'
    Write-Host ' Plugins are programs written by other people. Only install ones you trust.' -ForegroundColor DarkGray
    $c = (Read-Host 'Choose').Trim().ToLower()
    if ($c -eq 'b' -or $c -eq '') { return }
    if ($c -eq 'f') { Install-PluginFlow; Wait-Enter }
    if ($c -eq 'r') {
      if (Get-ServerProc) { Warn 'Stop the server first, then remove plugins.'; Wait-Enter; continue }
      $n = Read-Int 'Number of the plugin to remove' 1 ([math]::Max(1, $jars.Count))
      if ($null -ne $n -and $jars.Count -gt 0) { Remove-Item $jars[$n - 1].FullName -Force; Say 'Removed.'; Wait-Enter }
    }
  }
}

# ---------- network and ping ----------
function Test-WifiAdapter([string]$media, [string]$desc) { return (($media -match '802\.11') -or ($desc -match 'Wi-?Fi|Wireless|WLAN')) }
function Get-LinkInfo {
  if ($env:OS -ne 'Windows_NT') { return $null }
  try {
    $rt = Get-NetRoute -DestinationPrefix '0.0.0.0/0' | Sort-Object RouteMetric | Select-Object -First 1
    $ad = Get-NetAdapter -InterfaceIndex $rt.InterfaceIndex
    return @{ name = $ad.Name; wifi = (Test-WifiAdapter "$($ad.PhysicalMediaType)" "$($ad.InterfaceDescription)"); gateway = "$($rt.NextHop)"; speed = "$($ad.LinkSpeed)" }
  } catch { return $null }
}
function Get-PingStats($rtts, [int]$count) {
  $r = @($rtts)
  if ($r.Count -eq 0) { return @{ ok = $false; sent = $count; loss = 100 } }
  $avg = ($r | Measure-Object -Average).Average
  $min = ($r | Measure-Object -Minimum).Minimum
  $max = ($r | Measure-Object -Maximum).Maximum
  $jit = 0
  if ($r.Count -gt 1) {
    $d = @(); for ($i = 1; $i -lt $r.Count; $i++) { $d += [math]::Abs($r[$i] - $r[$i - 1]) }
    $jit = ($d | Measure-Object -Average).Average
  }
  return @{ ok = $true; sent = $count; avg = [math]::Round($avg, 1); min = $min; max = $max; jitter = [math]::Round($jit, 1); loss = [int][math]::Round(100 * ($count - $r.Count) / $count) }
}
function Get-PingGrade($st) {
  if (-not $st.ok) { return 'No reply' }
  if ($st.loss -ge 5) { return 'Poor (packet loss - expect rubber-banding)' }
  if ($st.avg -lt 30 -and $st.jitter -lt 10) { return 'Excellent' }
  if ($st.avg -lt 60 -and $st.jitter -lt 20) { return 'Good' }
  if ($st.avg -lt 100) { return 'OK (noticeable in fast games)' }
  return 'Poor (high delay)'
}
function Ping-Host([string]$target, [int]$count = 10) {
  $ping = New-Object System.Net.NetworkInformation.Ping
  $rtts = New-Object System.Collections.Generic.List[double]
  for ($i = 0; $i -lt $count; $i++) {
    try { $r = $ping.Send($target, 1500); if ($r.Status -eq 'Success') { $rtts.Add([double]$r.RoundtripTime) } } catch { }
    Start-Sleep -Milliseconds 150
  }
  return (Get-PingStats $rtts.ToArray() $count)
}
function Show-Ping($label, $target) {
  Write-Host ("  Testing {0} ({1})..." -f $label, $target) -NoNewline
  $st = Ping-Host $target 10
  Write-Host ''
  if ($st.ok) {
    Write-Host ("    average {0} ms   best {1}   worst {2}   jitter {3} ms   lost {4}%   -> {5}" -f $st.avg, $st.min, $st.max, $st.jitter, $st.loss, (Get-PingGrade $st))
  } else { Write-Host '    no reply (that address may block ping)' -ForegroundColor Yellow }
  return $st
}
function Add-UpnpForward([int]$port) {
  if ($env:OS -ne 'Windows_NT') { return @{ ok = $false; text = 'UPnP forwarding only works on Windows here.' } }
  try {
    $lan = Get-LanIp
    if (-not $lan) { return @{ ok = $false; text = 'Could not work out this PC''s home-network address.' } }
    $nat = New-Object -ComObject HNetCfg.NATUPnP
    $maps = $nat.StaticPortMappingCollection
    if ($null -eq $maps) { return @{ ok = $false; text = 'Your router did not answer. UPnP may be switched off in the router settings (or your provider blocks it).' } }
    [void]$maps.Add($port, 'TCP', $port, $lan, $true, 'Minecraft Server')
    return @{ ok = $true; text = "Forwarded TCP port $port to $lan." }
  } catch { return @{ ok = $false; text = "$($_.Exception.Message)" } }
}
function Remove-UpnpForward([int]$port) {
  if ($env:OS -ne 'Windows_NT') { return @{ ok = $false; text = 'Only on Windows.' } }
  try {
    $nat = New-Object -ComObject HNetCfg.NATUPnP
    if ($null -eq $nat.StaticPortMappingCollection) { return @{ ok = $false; text = 'Your router did not answer.' } }
    $nat.StaticPortMappingCollection.Remove($port, 'TCP')
    return @{ ok = $true; text = "Removed the forward for port $port." }
  } catch { return @{ ok = $false; text = "$($_.Exception.Message)" } }
}
function Add-FirewallRule([int]$port) {
  if ($env:OS -ne 'Windows_NT') { return @{ ok = $false; text = 'Only on Windows.' } }
  try {
    if (Get-NetFirewallRule -DisplayName 'Minecraft Server' -ErrorAction SilentlyContinue) { return @{ ok = $true; text = 'The Windows Firewall rule already exists.' } }
    [void](New-NetFirewallRule -DisplayName 'Minecraft Server' -Direction Inbound -Protocol TCP -LocalPort $port -Action Allow -Profile Private, Public)
    return @{ ok = $true; text = "Windows Firewall now allows port $port." }
  } catch { return @{ ok = $false; text = 'Could not add the Windows Firewall rule. Close this program, right-click the .bat and choose Run as administrator, then try again.' } }
}
function Parse-SchemeGuid([string]$t) {
  if ($t -match '([0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12})') { return $Matches[1] }
  return $null
}
function Enter-PerfMode {
  if ($env:OS -ne 'Windows_NT' -or $script:S['perfMode'] -eq $false) { return $null }
  try {
    $prev = Parse-SchemeGuid ((powercfg /getactivescheme) | Out-String)
    & powercfg /setactive 8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c *> $null
    return $prev
  } catch { return $null }
}
function Exit-PerfMode($prev) {
  if ($prev -and $env:OS -eq 'Windows_NT') { try { & powercfg /setactive $prev *> $null } catch { } }
}
function Menu-Network {
  while ($true) {
    Clear-Host
    Write-Host '--- Network and ping ---' -ForegroundColor Cyan
    $port = [int](Get-Prop 'server-port' '25565')
    $li = Get-LinkInfo
    if ($li) {
      Write-Host ("Your connection : {0} ({1}{2})" -f $li.name, $(if ($li.wifi) { 'Wi-Fi' } else { 'cable' }), $(if ($li.speed) { ", $($li.speed)" } else { '' }))
      if ($li.wifi) { Write-Host '  Wi-Fi adds random lag spikes. Plug this PC into the router with a cable for the best ping.' -ForegroundColor Yellow }
    }
    Write-Host ("Performance mode: {0}  (high-performance power plan while the server runs)" -f $(if ($script:S['perfMode'] -eq $false) { 'off' } else { 'on' }))
    Write-Host ''
    Write-Host ' 1) Connection check (delay, jitter and packet loss to the internet)'
    Write-Host ' 2) Ping a friend or any address'
    Write-Host ' 3) One-click port forward (UPnP) + firewall rule   <- best ping, no tunnel'
    Write-Host ' 4) Remove the port forward'
    Write-Host ' 5) Open my router page (for manual port forwarding)'
    Write-Host ' 6) Turn performance mode on/off'
    Write-Host ' B) Back'
    $c = (Read-Host 'Choose').Trim().ToLower()
    if ($c -eq 'b' -or $c -eq '') { return }
    switch ($c) {
      '1' {
        Write-Host ''
        $a = Show-Ping 'Cloudflare' '1.1.1.1'
        $b = Show-Ping 'Google' '8.8.8.8'
        Write-Host ''
        if (-not $a.ok -and -not $b.ok) { Warn 'No replies at all. Check your internet connection (some networks block ping).' }
        elseif ((($a.ok -and $a.loss -ge 5) -or ($b.ok -and $b.loss -ge 5))) { Warn 'Packet loss found. Try a cable instead of Wi-Fi, and check nobody is using the connection heavily.' }
        elseif ((($a.ok -and $a.jitter -ge 20) -or ($b.ok -and $b.jitter -ge 20))) { Warn 'High jitter (uneven delay). Wi-Fi or a busy connection is the usual cause.' }
        else { Say 'Your connection looks healthy for hosting.' }
        Write-Host 'In NZ a delay under about 30 ms to these is normal on fibre. Your friends'' ping to YOU also depends on where they are.' -ForegroundColor DarkGray
        Wait-Enter
      }
      '2' {
        $t = (Read-Host 'Address or IP to ping (Enter to cancel)').Trim()
        if ($t) {
          $t = ($t -replace ':\d+$', '')
          if ($t -match '^[A-Za-z0-9.\-]{1,100}$') { [void](Show-Ping 'target' $t); Write-Host 'Some addresses (like playit tunnels) ignore ping, so no reply does not always mean a problem.' -ForegroundColor DarkGray }
          else { Warn 'That does not look like an address.' }
          Wait-Enter
        }
      }
      '3' {
        $fw = Add-FirewallRule $port
        Write-Host ("  Firewall : {0}" -f $fw.text) -ForegroundColor $(if ($fw.ok) { 'Green' } else { 'Yellow' })
        $up = Add-UpnpForward $port
        Write-Host ("  Router   : {0}" -f $up.text) -ForegroundColor $(if ($up.ok) { 'Green' } else { 'Yellow' })
        if ($up.ok) {
          $ip = Get-PublicIp
          if ($ip) { Say "Friends can now join with: $ip$(if ($port -ne 25565) { ":$port" })" }
          Write-Host 'If that does not work, your provider may share one public IP between customers (CGNAT). Then keep using the playit tunnel.' -ForegroundColor DarkGray
        } else { Write-Host '  You can still forward the port by hand: press 5 for your router page.' }
        Wait-Enter
      }
      '4' { $r = Remove-UpnpForward $port; Write-Host ("  {0}" -f $r.text); Wait-Enter }
      '5' {
        $li2 = Get-LinkInfo
        $lan = Get-LanIp
        if ($li2 -and $li2.gateway) {
          Write-Host ("Opening http://{0} - log in with the details on your router's label." -f $li2.gateway)
          Write-Host "Find 'Port forwarding' or 'Virtual server' and add:  protocol TCP, external port $port, to $lan port $port."
          Start-Process ("http://" + $li2.gateway)
        } else { Warn 'Could not find your router. It is usually at 192.168.1.1 or 192.168.0.1.' }
        Wait-Enter
      }
      '6' { $script:S['perfMode'] = ($script:S['perfMode'] -eq $false); Save-Settings; Say "Performance mode is now $(if ($script:S['perfMode'] -eq $false) { 'off' } else { 'on' }). It applies the next time the server starts."; Wait-Enter }
    }
  }
}
function Apply-LowLag {
  Set-Prop 'view-distance' '6'; Set-Prop 'simulation-distance' '4'; Set-Prop 'sync-chunk-writes' 'false'
}
function Reset-Recommended {
  Set-Prop 'view-distance' "$($script:HW.view)"; Set-Prop 'simulation-distance' "$($script:HW.sim)"; Set-Prop 'sync-chunk-writes' 'true'
  Set-Prop 'max-players' "$($script:HW.players)"
}

function Panel {
  Write-Host 'Checking for updates...'
  Check-UpdateHint
  while ($true) {
    Show-Header
    $running = [bool](Get-ServerProc)
    if ($running) {
      Write-Host ' 1) Stop server (saves first)      2) Stop with 30-second warning to players'
      Write-Host ' 3) Restart server                 4) Force stop (only if stuck)'
    } else {
      Write-Host ' 1) Start server'
    }
    Write-Host ' 5) Players (allow in / admins)    6) Admin actions (kick, ban, message...)'
    Write-Host ' 7) Settings (RAM, auto-backup...) 8) Backups'
    Write-Host ' G) Game rules and new world       P) Plugins'
    Write-Host ' U) Updates                        I) Info, IP addresses and server log'
    Write-Host ' N) Network and ping'
    Write-Host ' 9) Open server folder             Q) Quit this panel (server keeps running)'
    $c = (Read-Host 'Choose').Trim().ToLower()
    switch ($c) {
      '1' { if ($running) { [void](Stop-ServerSafely 0) } else { Start-ServerWindow }; Wait-Enter }
      '2' { if ($running) { [void](Stop-ServerSafely 30); Wait-Enter } }
      '3' { if ($running) { if (Stop-ServerSafely 0) { Start-Sleep -Seconds 3; Start-ServerWindow }; Wait-Enter } }
      '4' { if ($running) { Force-Stop; Wait-Enter } }
      '5' { Menu-Players }
      '6' { Menu-Actions }
      '7' { Menu-Settings }
      '8' { Menu-Backups }
      'g' { Menu-Game }
      'p' { Menu-Plugins }
      'u' { Menu-Update }
      'i' { Menu-Info }
      'n' { Menu-Network }
      '9' { if ($env:OS -eq 'Windows_NT') { Start-Process explorer.exe $sd } }
      'q' { if ($running) { Write-Host 'The server keeps running in its own window.' }; return }
    }
  }
}
function Main {
  New-Item -ItemType Directory -Force -Path $sd | Out-Null
  $script:HW = Get-Hardware
  $script:S = Load-Settings
  if (-not (Test-Path $cfgPath)) {
    if ($env:BH_MODE -eq 'server') { throw 'Run the control panel first to set things up.' }
    First-TimeSetup
  }
  Ensure-Defaults
  if ($env:BH_MODE -eq 'server') { Run-Server } else { Panel }
}

if ($env:BH_TEST -ne '1') { Main }
