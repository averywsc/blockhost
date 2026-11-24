$env:BH_TEST='1'; $env:BH_ROOT='/tmp/bhp'; $env:BH_RAM='8192'; $env:BH_RCON_PORT='25599'
. $env:BH_PANEL
function Expand-Archive { param($Path,$DestinationPath,[switch]$Force) Add-Type -AssemblyName System.IO.Compression.FileSystem; [IO.Compression.ZipFile]::ExtractToDirectory($Path,$DestinationPath,$true) }  # sandbox shim: the real cmdlet cannot load its localized data in invariant-culture mode
$fail=0
function Check($name,$cond){ if($cond){"PASS $name"}else{"FAIL $name"; $script:fail++} }

$hw = Get-Hardware; Check 'hardware 8GB' ($hw.heap -eq 4915 -and $hw.players -eq 16)
$env:BH_RAM='2048'; $hw2=Get-Hardware; Check 'hardware 2GB floor' ($hw2.heap -eq 1024 -and $hw2.players -eq 4); $env:BH_RAM='8192'

$script:HW=$hw; $script:S=@{ javaVersion=21; minecraft='1.21.10'; rconPassword='secret123' }
Save-Settings
$script:S = Load-Settings; Ensure-Defaults
Check 'settings migrate' ($script:S.heapMb -eq 4915 -and $script:S.tunnel -eq $false -and $script:S.rconPassword -eq 'secret123')

Set-Prop 'max-players' '12'; Set-Prop 'motd' 'Hi there'; Set-Prop 'max-players' '20'
Check 'props' ((Get-Prop 'max-players' '?') -eq '20' -and (Get-Prop 'motd' '') -eq 'Hi there' -and (Get-Prop 'nope' 'd') -eq 'd')
Check 'props no dup' ((Get-Content "$sd/server.properties" | Where-Object {$_ -like 'max-players=*'}).Count -eq 1)

$r = Rcon 'list'; Check 'rcon list' ($r.ok -and $r.text -eq 'OK There are 2 of a max of 8 players online: Alice, Bob')
$r = Rcon 'say hello world'; Check 'rcon say' ($r.ok -and $r.text -eq 'OK ran:say hello world')
$script:S.rconPassword='wrong'; $r=Rcon 'list'; Check 'rcon bad pw' (-not $r.ok -and $r.text -match 'bad password'); $script:S.rconPassword='secret123'
$old=$RconPort; $script:RconPort=1; $r=Rcon 'list'; Check 'rcon down' (-not $r.ok); $script:RconPort=$old

function Get-Uuid($n){ return '11111111-2222-3333-4444-555555555555' }
Add-Player 'Steve_1' $true; Add-Player 'Alex' $false; Add-Player 'bad name!' $false
Check 'offline add' ((Read-List 'whitelist.json').Count -eq 2 -and (Read-List 'ops.json').Count -eq 1)
Add-Player 'Steve_1' $true; Check 'no dup' ((Read-List 'whitelist.json').Count -eq 2)
Remove-Player 'Alex' $false; Check 'remove' ((Read-List 'whitelist.json').Count -eq 1)
Remove-Player 'Steve_1' $true; Check 'deop' ((Read-List 'ops.json').Count -eq 0 -and (Read-List 'whitelist.json').Count -eq 1)
Show-Players

# backup / restore roundtrip
New-Item -ItemType Directory -Force "$sd/world/region","$sd/world_nether" | Out-Null
Set-Content "$sd/world/region/r.0.0.mca" 'original'; Set-Content "$sd/world/session.lock" 'x'
$z = New-Backup; Check 'backup made' ($z -and (Test-Path $z))
Add-Type -AssemblyName System.IO.Compression.FileSystem
$names = [IO.Compression.ZipFile]::OpenRead($z).Entries | ForEach-Object { $_.FullName }
Check 'zip entries' (($names -contains 'world/region/r.0.0.mca') -and -not ($names -match 'session.lock'))
Set-Content "$sd/world/region/r.0.0.mca" 'griefed'
$aside = Restore-Backup $z
Check 'restore' ((Get-Content "$sd/world/region/r.0.0.mca") -eq 'original' -and (Get-Content "$aside/world/region/r.0.0.mca") -eq 'griefed')
for($i=0;$i -lt 12;$i++){ Start-Sleep -Milliseconds 1100; [void](New-Backup) }
Check 'prune to 10' ((Get-ChildItem $bk -Filter *.zip).Count -eq 10)
"FAILS: $fail"
