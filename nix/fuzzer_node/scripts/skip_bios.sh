#!/usr/bin/env bash

PATH=$PATH:/run/current-system/sw/bin/

# LEFT
send_keys left

for _i in {1..3}; do
    send_keys down
done

send_keys enter
