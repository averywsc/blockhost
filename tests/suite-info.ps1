$env:BH_TEST='1'; $env:BH_ROOT='/tmp/bhi'; $env:BH_RAM='8192'; $env:BH_RCON_PORT='25598'
. $env:BH_PANEL
$script:HW = Get-Hardware; $script:S = Load-Settings; Ensure-Defaults
function Get-ServerProc { return (Get-Process -Id $PID) }   # pretend the server is running
function Get-PublicIp { return '203.0.113.7' }               # no internet in the sandbox
function Set-Clipboard { param($Value) $script:clip = $Value }
Menu-Info
"clipboard: $script:clip"
"saved: $((Get-Content /tmp/bhi/settings.json -Raw | ConvertFrom-Json).playitAddress)"
Show-Header
