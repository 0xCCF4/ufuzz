#!/usr/bin/env bash

# https://usb.org/sites/default/files/hut1_21.pdf - Chapter 10 Keyboard/Keypad Page (0x07)

send_keycode() {
    local keycode=$1
    echo -ne "\0\0\x${keycode}\0\0\0\0\0" > /dev/hidg0
    sleep 0.1
    echo -ne "\0\0\0\0\0\0\0\0" > /dev/hidg0
    sleep 0.1
}

enter() {
    send_keycode "28"
}

left() {
    send_keycode "50"
}

down() {
    send_keycode "51"
}

# LEFT
left

for _i in {1..3}; do
    # DOWN
    down
done

# ENTER
enter
