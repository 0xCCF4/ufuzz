#!/usr/bin/env bash

# https://usb.org/sites/default/files/hut1_21.pdf - Chapter 10 Keyboard/Keypad Page (0x07)

send_keycode() {
    local keycode=$1
    echo -ne "\0\0\x${keycode}\0\0\0\0\0" > /dev/hidg0
    sleep 0.1
    echo -ne "\0\0\0\0\0\0\0\0" > /dev/hidg0
    sleep 0.1
}

send_string() {
    local string=$1
    for (( i=0; i<${#string}; i++ )); do
        char="${string:$i:1}"
        case "$char" in
            [a-z]) keycode=$(printf "%x" $(( $(printf "%d" "'$char") - 97 + 4 )) ) ;;
            [A-Z]) keycode=$(printf "%x" $(( $(printf "%d" "'$char") - 65 + 4 )) ) ;;
            [1-9]) keycode=$(printf "%x" $(( $(printf "%d" "'$char") - 49 + 0x1E )) ) ;;
            "0") keycode=27 ;;
            " ") keycode=2c ;;
            "-") keycode=2d ;;
            ":") keycode=33 ;;
            *) continue ;;
        esac
        send_keycode "$keycode"
    done
}

enter() {
    send_keycode "28"
}

send_string "map -r"
enter

send_string "fs0:"
enter

send_string "startup"
enter