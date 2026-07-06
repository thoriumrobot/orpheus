#!/bin/bash
# Cross-architecture interop: PC (x86-64) <-> "phone" (ARM64 under qemu, no rustc,
# isolated HOME) — the same binary that runs on Android. Four directions tested.
cd /home/claude/work/orpheus-linux-x86_64
PC=$PWD/target/release/latte
ARM=$PWD/target/aarch64-unknown-linux-gnu/release/latte
QEMU="/usr/bin/qemu-aarch64 -L /usr/aarch64-linux-gnu"

rm -rf /tmp/armhome /tmp/pchome /tmp/xstore-pc /tmp/xstore-arm /tmp/xmodel
mkdir -p /tmp/armhome/tmp /tmp/armhome/bin /tmp/pchome
AENV="HOME=/tmp/armhome TMPDIR=/tmp/armhome/tmp ORPHEUS_CACHE=/tmp/armhome/cache PATH=/tmp/armhome/bin"
PENV="HOME=/tmp/pchome ORPHEUS_CACHE=/tmp/pchome/cache"

echo "=== T1: PC coordinator -> ARM worker ==="
setsid env $AENV $QEMU $ARM worker --listen 127.0.0.1:9711 > /tmp/w_arm.log 2>&1 < /dev/null &
sleep 2
env $PENV ORPHEUS_WORKERS=127.0.0.1:9711 $PC eval "(dmap (fn [x] -> (mul x x)) [ 3 [ 4 [ 5 0 ] ] ])" 2>&1
grep -c "task:" /tmp/w_arm.log | sed 's/^/  arm worker executed tasks: /'

echo "=== T2: ARM coordinator -> PC worker ==="
env $PENV setsid $PC worker --listen 127.0.0.1:9712 > /tmp/w_pc.log 2>&1 < /dev/null &
sleep 1
env $AENV ORPHEUS_WORKERS=127.0.0.1:9712 $QEMU $ARM eval "(dmap (fn [x] -> +(x)) [ 10 [ 20 [ 30 0 ] ] ])" 2>&1
grep -c "task:" /tmp/w_pc.log | sed 's/^/  pc worker executed tasks: /'

echo "=== T3: gossip convergence PC <-> ARM (kv agent, durable stores) ==="
env $PENV setsid $PC node --listen 127.0.0.1:9713 --agent kv --id 1 --store /tmp/xstore-pc --do "put pc 42" --run-secs 18 > /tmp/n_pc.log 2>&1 < /dev/null &
sleep 1
env $AENV $QEMU $ARM node --listen 127.0.0.1:9714 --peer 127.0.0.1:9713 --agent kv --id 2 --store /tmp/xstore-arm --do "put arm 7" --run-secs 10 > /tmp/n_arm.log 2>&1
for i in $(seq 1 14); do grep -q FINAL /tmp/n_pc.log && break; sleep 1; done
echo "  PC : $(grep FINAL /tmp/n_pc.log)"
echo "  ARM: $(grep FINAL /tmp/n_arm.log)"

echo "=== T4: distributed FedAvg training across BOTH architectures ==="
env $PENV ORPHEUS_WORKERS=127.0.0.1:9711,127.0.0.1:9712 $PC ml linear --rounds 2 --local-iters 150 --store /tmp/xmodel 2>&1 | grep -E "workers:|round|learned|fallbacks|persistent"
echo "  arm worker total tasks: $(grep -c 'task:' /tmp/w_arm.log)   pc worker total tasks: $(grep -c 'task:' /tmp/w_pc.log)"

echo "=== T5: the phone profile — no rustc, dist detection still on ==="
env $AENV ORPHEUS_WORKERS=127.0.0.1:9712 $QEMU $ARM profile "(dmap (fn [x] -> (mul x x)) [ 1 [ 2 0 ] ])" 2>&1 | grep -E "native|distributable|dist decision"

pkill -x latte 2>/dev/null
pkill -f qemu-aarch64 2>/dev/null
echo "=== ALL DONE ==="
