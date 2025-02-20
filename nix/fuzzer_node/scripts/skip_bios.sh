#!/usr/bin/env bash

# LEFT
send_keys left

for _i in {1..3}; do
    send_keys down
done

send_keys enter
