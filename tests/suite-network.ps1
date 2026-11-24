$env:BH_TEST='1'; $env:BH_ROOT='/tmp/bh5'; $env:BH_RAM='8192'; $env:BH_STARTUP='/tmp/bh5'
. $env:BH_PANEL
$fail=0; function Check($n,$c){ if($c){"PASS $n"}else{"FAIL $n"; $script:fail++} }
$script:HW=Get-Hardware; $script:S=Load-Settings; Ensure-Defaults
$script:Q = New-Object System.Collections.Queue; $script:reads=0
function Read-Host { param($Prompt) $script:reads++; if($script:reads -gt 200){throw 'too many prompts'}; if($script:Q.Count -gt 0){ return [string]$script:Q.Dequeue() } else { return '' } }
function In([string[]]$x){ foreach($i in $x){ $script:Q.Enqueue($i) } }
function Clear-Host {}

$st = Get-PingStats @(10.0,12.0,11.0,30.0) 4
Check 'stats avg/min/max' ($st.avg -eq 15.8 -and $st.min -eq 10 -and $st.max -eq 30 -and $st.loss -eq 0)
Check 'stats jitter' ($st.jitter -eq 7.3)

Check 'stats loss' ((Get-PingStats @(20.0,22.0) 10).loss -eq 80)
Check 'stats none' (-not (Get-PingStats @() 10).ok)
Check 'grade excellent' ((Get-PingGrade (Get-PingStats @(10.0,11.0,10.0) 3)) -eq 'Excellent')
Check 'grade loss' ((Get-PingGrade (Get-PingStats @(10.0,11.0) 10)) -like 'Poor (packet loss*')
Check 'grade high delay' ((Get-PingGrade (Get-PingStats @(150.0,152.0,151.0) 3)) -like 'Poor (high delay*')
Check 'wifi detect' ((Test-WifiAdapter 'Native 802.11' 'Intel AX201') -and (Test-WifiAdapter '' 'Realtek Wireless LAN') -and -not (Test-WifiAdapter '802.3' 'Realtek PCIe GbE'))
Check 'scheme guid parse' ((Parse-SchemeGuid 'Power Scheme GUID: 381b4222-f694-41f0-9685-ff5bb260df2e  (Balanced)') -eq '381b4222-f694-41f0-9685-ff5bb260df2e')
Check 'scheme guid none' ($null -eq (Parse-SchemeGuid 'nothing here'))
$r = Ping-Host '127.0.0.1' 2; Check 'ping-host returns stats (loopback or no-ping sandbox)' ($r.ContainsKey('ok'))
Check 'perfmode noop off-windows' ($null -eq (Enter-PerfMode))
Exit-PerfMode $null

# low-lag / reset
Apply-LowLag; Check 'low lag' ((Get-Prop 'view-distance' '') -eq '6' -and (Get-Prop 'simulation-distance' '') -eq '4' -and (Get-Prop 'sync-chunk-writes' '') -eq 'false')
Reset-Recommended; Check 'reset' ((Get-Prop 'view-distance' '') -eq '10' -and (Get-Prop 'sync-chunk-writes' '') -eq 'true' -and (Get-Prop 'max-players' '') -eq '16')
In 'l','','b'; Menu-Settings; Check 'settings L' ((Get-Prop 'view-distance' '') -eq '6')
In 'r','','b'; Menu-Settings; Check 'settings R' ((Get-Prop 'view-distance' '') -eq '10')

# network menu (mock the pings so the sandbox needs no ICMP)
function Show-Ping($label,$target){ Write-Host "  mock ping $label $target"; return (Get-PingStats @(12.0,14.0,13.0,12.0) 4) }
In '1','','2','not an addr!!','','2','friend.example.com:25565','','6','','6','','b'; Menu-Network
Check 'perf mode toggled twice -> on' ($script:S['perfMode'] -ne $false)
In '3','','4','','5','','b'; Menu-Network   # UPnP/firewall/router are Windows-only: must degrade gracefully
Check 'upnp degrades' ((Add-UpnpForward 25565).ok -eq $false)
"FAILS: $fail"
