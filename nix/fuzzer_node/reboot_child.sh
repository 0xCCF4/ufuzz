#!/usr/bin/env bash

power_button() {
    local PRESS="${1:-}"


    if [ -n "$PRESS" ]; then
        echo "Pressing power button"
        pinctrl set 0 op pn dh
    else
        echo "Releasing power button"
        pinctrl set 0 ip pd
    fi
}

force_shutdown() {
    echo "Forcing shutdown"
    power_button press
    sleep 10
    power_button
    sleep 1
}

start() {
    power_button press
    sleep 1
    power_button
    sleep 1
}

boot() {
    if configure_usb check; then
        configure_usb off
    fi
    sleep 2
    start
    echo "Waiting for BIOS"
    sleep 50
    configure_usb on
    sleep 2
    echo "Selecting boot device"
    skip_bios
    sleep 1
    configure_usb off
    echo "Booting"
    sleep 2
    configure_usb on "$1"
    sleep 2
}

if [ "$1" != "halt" ] && [ -z "${2:-}" ]; then
    echo "Usage: $0 {reboot|start|halt} FILE"
    exit 1
fi

case "${1:-}" in
    reboot)
        force_shutdown
        if configure_usb check; then
            configure_usb off
            sleep 2
        fi
        boot "$2"
        ;;
    start)  
        boot "$2"
        ;;
    halt)
        force_shutdown
        ;;
    *)
        echo "Usage: $0 {reboot|start|halt} FILE"
        exit 1
        ;;
esac


