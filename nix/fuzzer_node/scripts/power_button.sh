#!/usr/bin/env bash

press() {
    sudo pinctrl set 0 op pn dh
}

release() {
    sudo pinctrl set 0 ip pd
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
        sync
        press
        sleep 14
        release
        ;;
    short)
        echo "Short press power button"
        sync
        press
        sleep 1
        release
        ;;
    *)
        echo "Usage: $0 {press|release|long|short}"
        exit 1
        ;;
esac
