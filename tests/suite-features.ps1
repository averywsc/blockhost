$env:BH_TEST='1'; $env:BH_ROOT='/tmp/bh3'; $env:BH_RAM='8192'; $env:BH_RCON_PORT='25596'; $env:BH_MODRINTH='http://127.0.0.1:25597'
$env:BH_STARTUP='/tmp/bh3/startup'; $env:BH_BAT='/tmp/bh3/Minecraft-Server.bat'
. $env:BH_PANEL
$fail=0; function Check($n,$c){ if($c){"PASS $n"}else{"FAIL $n"; $script:fail++} }
$script:HW=Get-Hardware; $script:S=Load-Settings; Ensure-Defaults
# scripted Read-Host
$script:Q = New-Object System.Collections.Queue; $script:reads=0
function Read-Host { param($Prompt) $script:reads++; if($script:reads -gt 300){throw 'too many prompts'}; if($script:Q.Count -gt 0){ return [string]$script:Q.Dequeue() } else { return '' } }
function In([string[]]$x){ foreach($i in $x){ $script:Q.Enqueue($i) } }
function Clear-Host {}

# ---- mock Paper API ----
$jarBytes = [Text.Encoding]::ASCII.GetBytes('new-paper-jar'); $sha = ([BitConverter]::ToString([Security.Cryptography.SHA256]::Create().ComputeHash($jarBytes)) -replace '-','')
function Mk($id,$ch){ @{ id=$id; channel=$ch; downloads=@{ 'server:default'=@{ url="https://example/paper-$id.jar"; checksums=@{ sha256=$sha } } } } }
$script:withMajor = $false
function Get-Json($url) {
  if ($url -eq 'https://fill.papermc.io/v3/projects/paper') { $v=@{ '1.21'=@('1.21.10','1.21.9'); '26.1'=@('26.1','26.1-rc1') }; if(-not $script:withMajor){ $v.Remove('26.1') }; return [pscustomobject]@{ versions=[pscustomobject]$v } }
  if ($url -match '/versions/26\.1/builds$') { if($script:withMajor){ return @((Mk 7 'STABLE')) } else { return @() } }
  if ($url -match '/versions/1\.21\.10/builds$') { return @((Mk 100 'STABLE'),(Mk 130 'STABLE'),(Mk 140 'ALPHA')) }
  if ($url -match '/versions/26\.1$') { return [pscustomobject]@{ version=[pscustomobject]@{ java=[pscustomobject]@{ version=[pscustomobject]@{ minimum=25 } } } } }
  if ($url -match '/versions/1\.21\.10$') { return [pscustomobject]@{ version=[pscustomobject]@{ java=[pscustomobject]@{ version=[pscustomobject]@{ minimum=21 } } } } }
  throw "unmocked $url"
}
function Get-File($url,$dest) { if ($url -like 'http://127.0.0.1*') { Invoke-WebRequest -Uri $url -OutFile $dest -UserAgent $UA } else { [IO.File]::WriteAllBytes($dest,$jarBytes) } }
function Get-JavaExe($jv) { return "fake-java-$jv" }

$p = Find-Paper $null; Check 'find latest stable (skips alpha/rc, picks max id)' ($p.v -eq '1.21.10' -and $p.b.id -eq 130 -and $p.jv -eq 21)
$script:withMajor=$true; $p2 = Find-Paper $null; Check 'finds new major w/ java 25' ($p2.v -eq '26.1' -and $p2.jv -eq 25); $script:withMajor=$false

# ---- update: same-version build ----
In '1'; Menu-Update
Check 'update build installed' ($script:S.paperBuild -eq 130 -and (Test-Path /tmp/bh3/server/server.jar) -and ((Get-Content /tmp/bh3/settings.json -Raw|ConvertFrom-Json).paperBuild -eq 130))
Check 'backup made before update' ((Get-ChildItem /tmp/bh3/backups -Filter *.zip).Count -ge 1)
In ''; Menu-Update; Check 'up to date path' ($script:UpdateHint -eq $null)
# ---- update: major ----
$script:withMajor=$true; In '2','nope'; Menu-Update; Check 'major needs YES' ($script:S.minecraft -eq '1.21.10')
In '2','YES'; Menu-Update; Check 'major upgrade' ($script:S.minecraft -eq '26.1' -and $script:S.javaVersion -eq 25 -and (Test-Path /tmp/bh3/server/server.jar.old))
$script:S['minecraft']='1.21.10'; $script:S['javaVersion']=21; $script:S['paperBuild']=130; Save-Settings; $script:withMajor=$false
Check-UpdateHint; Check 'no hint when current' ($script:UpdateHint -eq $null)
$script:S['paperBuild']=100; Check-UpdateHint; Check 'hint when behind' ($script:UpdateHint -like '*Update available*'); $script:S['paperBuild']=130

# ---- autostart ----
Set-AutoStart $true; Check 'autostart on' ((Test-AutoStart) -and ((Get-Content (Get-StartupFile) -Raw) -match 'start "" "/tmp/bh3/Minecraft-Server.bat" server'))
Set-AutoStart $false; Check 'autostart off' (-not (Test-AutoStart))

# ---- settings menu ----
In '6','12','','7','3','','9','YES','','b'; Menu-Settings
Check 'restart/backup hours saved' ($script:S.restartHours -eq 12 -and $script:S.backupHours -eq 3)
Check 'whitelist off' ((Get-Prop 'white-list' '') -eq 'false' -and (Get-Prop 'enforce-whitelist' '') -eq 'false')
In '9','','b'; Menu-Settings; Check 'whitelist back on' ((Get-Prop 'white-list' '') -eq 'true')
In '1','99','','b'; Menu-Settings; Check 'ram capped to ram-1GB' ($script:S.heapMb -eq 7168)
In '1','2.5','','b'; Menu-Settings; Check 'ram 2.5GB' ($script:S.heapMb -eq 2560)
In '2','12','','3','40','b'; Menu-Settings; Check 'max players ok, view out of range ignored' ((Get-Prop 'max-players' '') -eq '12' -and (Get-Prop 'view-distance' 'none') -eq 'none')
$script:S['restartHours']=0; $script:S['backupHours']=6; Save-Settings

# ---- game rules / new world ----
In '1','4','','2','2','','3','2','','b'; Menu-Game
Check 'difficulty/gamemode/pvp' ((Get-Prop 'difficulty' '') -eq 'hard' -and (Get-Prop 'gamemode' '') -eq 'creative' -and (Get-Prop 'pvp' '') -eq 'false')
In '5','no','','b'; Menu-Game; Check 'new world needs YES' (Test-Path /tmp/bh3/server/world)
In '5','YES','my seed 1','','b'; Menu-Game
Check 'new world moved old + seed' (-not (Test-Path /tmp/bh3/server/world) -and (Get-ChildItem /tmp/bh3/old-worlds -Recurse -Filter level.dat) -and (Get-Prop 'level-seed' '') -eq 'my seed 1')
# ---- plugins ----
In 'f','chunky','1','y','n','b'; Menu-Plugins
Check 'plugin installed + hash verified' ((Get-Content /tmp/bh3/server/plugins/Chunky-1.4.0.jar -Raw) -eq 'PK-fake-plugin-bytes')
In 'r','1','b'; Menu-Plugins; Check 'plugin removed' (-not (Test-Path /tmp/bh3/server/plugins/Chunky-1.4.0.jar))
"FAILS: $fail"
