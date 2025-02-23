#!/usr/bin/env bash

PATH=$PATH:/run/current-system/sw/bin/

# LEFT
send_keys left
sleep 0.3s

for _i in {1..3}; do
    send_keys down
    sleep 0.3s
done

send_keys enter
