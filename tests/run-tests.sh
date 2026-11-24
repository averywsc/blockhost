#!/usr/bin/env bash
# Runs every suite against ../panel.ps1 using PowerShell on Linux/macOS plus fake servers.
# Usage:  PWSH=/path/to/pwsh ./run-tests.sh      (needs python3)
# NOTE: do not use `pkill -f pwsh` in your own shell commands - it can match and kill the shell itself.
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
export BH_PANEL="$HERE/../panel.ps1"
export DOTNET_SYSTEM_GLOBALIZATION_INVARIANT=1   # sandboxes without ICU; see HANDOFF.md "Test gotchas"
PWSH=${PWSH:-pwsh}
for p in 25595 25596 25598 25599; do setsid python3 "$HERE/fakercon.py" $p >/dev/null 2>&1 </dev/null & done
setsid python3 "$HERE/fakemod.py" 25597 >/dev/null 2>&1 </dev/null &
sleep 1.5
cleanup() { pkill -f 'fakerco[n].py'; pkill -f 'fakemo[d].py'; }
trap cleanup EXIT
fails=0
run() { # name file [stdin]
  local name=$1 file=$2 input=${3-}
  local out; out=$(printf "$input" | timeout 180 "$PWSH" -NoProfile -File "$HERE/$file" 2>&1 | sed 's/\x1b\[[0-9;]*[A-Za-z]//g')
  local pass; pass=$(echo "$out" | grep -c '^PASS')
  local bad; bad=$(echo "$out" | grep -c '^FAIL ')
  local summary; summary=$(echo "$out" | grep '^FAILS')
  if [ "$bad" -gt 0 ] || { [ -n "$summary" ] && [ "$summary" != "FAILS: 0" ]; }; then echo "[FAIL] $name ($pass passed)"; echo "$out" | grep -E '^FAIL |rror|xception' | head; fails=$((fails+1)); else echo "[ok]   $name ($pass checks)"; fi
}
# base: hardware, settings, properties, RCON, whitelist, backup/restore
rm -rf /tmp/bhp; mkdir -p /tmp/bhp/server
run base suite-base.ps1
# features: updates, settings, game rules, new world, autostart, plugins
rm -rf /tmp/bh3; mkdir -p /tmp/bh3/server/world /tmp/bh3/startup; echo data > /tmp/bh3/server/world/level.dat
printf 'server-port=25565\nmax-players=8\nwhite-list=true\nenforce-whitelist=true\ndifficulty=easy\n' > /tmp/bh3/server/server.properties
echo '{"javaVersion":21,"minecraft":"1.21.10","paperBuild":100,"tunnel":false,"heapMb":3072,"rconPassword":"secret123","backupHours":6,"restartHours":0}' > /tmp/bh3/settings.json
run features suite-features.ps1
# network, ping, performance mode, low-lag
rm -rf /tmp/bh5; mkdir -p /tmp/bh5/server; printf 'server-port=25565\nmax-players=8\nview-distance=10\n' > /tmp/bh5/server/server.properties
echo '{"javaVersion":21,"minecraft":"1.21.10","paperBuild":100,"tunnel":false,"heapMb":3072,"rconPassword":"x","backupHours":6,"restartHours":0}' > /tmp/bh5/settings.json
run network suite-network.ps1
# server window loop with a fake java: scheduled restart, timed backup, stop flag
rm -rf /tmp/bh4; mkdir -p /tmp/bh4/server/world /tmp/bh4/java-21/jre/bin; echo data > /tmp/bh4/server/world/level.dat
cat > /tmp/bh4/java-21/jre/bin/java.exe <<'JEOF'
#!/bin/bash
C=/tmp/bh4/count; n=$(( $(cat $C 2>/dev/null || echo 0) + 1 )); echo $n > $C
echo "fake paper run $n: $@"
if [ $n -eq 1 ]; then sleep 4; exit 0; fi
touch /tmp/bh4/stop.flag; sleep 1; exit 0
JEOF
chmod +x /tmp/bh4/java-21/jre/bin/java.exe
echo '{"javaVersion":21,"minecraft":"1.21.10","paperBuild":100,"tunnel":false,"heapMb":1024,"rconPassword":"secret123","backupHours":0.0002,"restartHours":0.0003}' > /tmp/bh4/settings.json
out=$(timeout 90 "$PWSH" -NoProfile -File "$HERE/suite-serverloop.ps1" 2>&1 | sed 's/\x1b\[[0-9;]*[A-Za-z]//g')
if echo "$out" | grep -q '^runs: 2' && echo "$out" | grep -q 'stopflag left: False'; then echo "[ok]   serverloop"; else echo "[FAIL] serverloop"; echo "$out" | tail -5; fails=$((fails+1)); fi
# info screen
rm -rf /tmp/bhi; mkdir -p /tmp/bhi/server/world /tmp/bhi/server/logs; echo x > /tmp/bhi/server/world/level.dat; printf 'a\nb\n' > /tmp/bhi/server/logs/latest.log; printf 'server-port=25565\nmax-players=8\n' > /tmp/bhi/server/server.properties
echo '{"javaVersion":21,"minecraft":"1.21.10","paperBuild":99,"tunnel":true,"heapMb":3072,"rconPassword":"secret123"}' > /tmp/bhi/settings.json
out=$(printf 'p\nmyserver.joinmc.link\n\nb\n' | timeout 60 "$PWSH" -NoProfile -File "$HERE/suite-info.ps1" 2>&1 | sed 's/\x1b\[[0-9;]*[A-Za-z]//g')
if echo "$out" | grep -q 'saved: myserver.joinmc.link'; then echo "[ok]   info"; else echo "[FAIL] info"; fails=$((fails+1)); fi
echo; [ $fails -eq 0 ] && echo "ALL SUITES PASSED" || echo "$fails suite(s) failed"; exit $fails
