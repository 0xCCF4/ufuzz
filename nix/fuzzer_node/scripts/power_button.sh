#!/usr/bin/env bash

press() {
    pinctrl set 0 op pn dh
}

release() {
    pinctrl set 0 ip pd
}

case "${1:-}" in
    press)
        echo "Pressing power button"
        press
        ;;
    release)
        echo "Releasing power button"
        release
        ;;
    long)
        echo "Long press power button"
        press
        sleep 10
        release
        ;;
    short)
        echo "Short press power button"
        press
        sleep 1
        release
        ;;
    *)
        echo "Invalid argument: $PRESS"
        echo "Usage: power_button {press|release|long|short}"
        exit 1
        ;;
esac
