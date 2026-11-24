$env:BH_TEST='1'; $env:BH_ROOT='/tmp/bh4'; $env:BH_RAM='8192'; $env:BH_RCON_PORT='25595'; $env:BH_TICK='1'; $env:BH_WARN='1'
. $env:BH_PANEL
$script:HW=Get-Hardware; $script:S=Load-Settings; Ensure-Defaults
function Get-ServerProc { return (Get-Process -Id $PID) }
Run-Server
"runs: $(Get-Content /tmp/bh4/count)"
"stopflag left: $(Test-Path /tmp/bh4/stop.flag)"
"backups: $((Get-ChildItem /tmp/bh4/backups -Filter *.zip -ErrorAction SilentlyContinue).Count)"
"rcon props: " + ((Get-Content /tmp/bh4/server/server.properties) -join ' ')
