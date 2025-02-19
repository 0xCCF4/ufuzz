#!/usr/bin/env bash

# https://usb.org/sites/default/files/hut1_21.pdf - Chapter 10 Keyboard/Keypad Page (0x07)

send_keycode() {
    local keycode=$1
    local modifier=${2:-0}
    echo -ne "\x${modifier}\0\x${keycode}\0\0\0\0\0" > /dev/hidg0
}

send_string() {
    local string=$1
    for (( i=0; i<${#string}; i++ )); do
        char="${string:$i:1}"
        modifier="0"
        case "$char" in
            [a-z]) keycode=$(printf "%x" $(( $(printf "%d" "'$char") - 97 + 4 )) ) ;;
            [A-Z]) keycode=$(printf "%x" $(( $(printf "%d" "'$char") - 65 + 4 )) ) ;;
            [1-9]) keycode=$(printf "%x" $(( $(printf "%d" "'$char") - 49 + 0x1E )) ) ;;
            "0") keycode=27 ;;
            " ") keycode=2c ;;
            "-") keycode=2d ;;
            ":") keycode=33 modifier=2 ;;
            *) continue ;;
        esac
        send_keycode "$keycode" "${modifier:-}"
        sleep 0.3s
        echo -n "$char"
    done
    send_keycode "0"
}

enter() {
    send_keycode "28"
    sleep 0.1s
    send_keycode "0"
    sleep 1s
    send_keycode "0"
    sleep 1s
    echo
}

set -x

send_string "map -r"
enter

send_string "fs0:"
enter

send_string "startup"
enter