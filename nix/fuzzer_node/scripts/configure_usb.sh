#!/usr/bin/env bash

# https://www.kernel.org/doc/Documentation/usb/gadget_configfs.txt

GADGET_DIR="/sys/kernel/config/usb_gadget/fuzz"

exists_gadget() {
    [ -d "$GADGET_DIR" ]
}

enable_gadget() {
    local FILE=${1:-}

    modprobe libcomposite
    sleep 2

    # https://www.isticktoit.net/?p=1383

    mkdir -p "$GADGET_DIR"
    echo 0x1d6b > "$GADGET_DIR/idVendor" # Linux Foundation
    echo 0x0104 > "$GADGET_DIR/idProduct" # Multifunction Composite Gadget
    echo 0x0100 > "$GADGET_DIR/bcdDevice" # v1.0.0
    echo 0x0200 > "$GADGET_DIR/bcdUSB" # USB2
    mkdir -p "$GADGET_DIR/strings/0x409"
    echo "0000000000000124" > "$GADGET_DIR/strings/0x409/serialnumber"
    echo "Fuzz" > "$GADGET_DIR/strings/0x409/manufacturer"
    echo "Fuzz USB Device" > "$GADGET_DIR/strings/0x409/product"
    mkdir -p "$GADGET_DIR/configs/c.1/strings/0x409"
    echo "Standard config" > "$GADGET_DIR/configs/c.1/strings/0x409/configuration"
    echo 250 > "$GADGET_DIR/configs/c.1/MaxPower"
    # Add functions here

    # HID keyboard
    mkdir -p "$GADGET_DIR/functions/hid.usb0"
    echo 1 > "$GADGET_DIR/functions/hid.usb0/protocol"
    echo 1 > "$GADGET_DIR/functions/hid.usb0/subclass"
    echo 8 > "$GADGET_DIR/functions/hid.usb0/report_length"
    echo -ne \\x05\\x01\\x09\\x06\\xa1\\x01\\x05\\x07\\x19\\xe0\\x29\\xe7\\x15\\x00\\x25\\x01\\x75\\x01\\x95\\x08\\x81\\x02\\x95\\x01\\x75\\x08\\x81\\x03\\x95\\x05\\x75\\x01\\x05\\x08\\x19\\x01\\x29\\x05\\x91\\x02\\x95\\x01\\x75\\x03\\x91\\x03\\x95\\x06\\x75\\x08\\x15\\x00\\x25\\x65\\x05\\x07\\x19\\x00\\x29\\x65\\x81\\x00\\xc0 > "$GADGET_DIR/functions/hid.usb0/report_desc"
    ln -s "$GADGET_DIR/functions/hid.usb0" "$GADGET_DIR/configs/c.1/"

    if [ -n "${FILE:-}" ]; then
        # Mass storage
        mkdir -p "$GADGET_DIR/functions/mass_storage.usb0"
        echo 1 > "$GADGET_DIR/functions/mass_storage.usb0/stall"
        echo 0 > "$GADGET_DIR/functions/mass_storage.usb0/lun.0/cdrom"
        echo 0 > "$GADGET_DIR/functions/mass_storage.usb0/lun.0/ro"
        echo 0 > "$GADGET_DIR/functions/mass_storage.usb0/lun.0/nofua"
        echo "$FILE" > "$GADGET_DIR/functions/mass_storage.usb0/lun.0/file"
        ln -s "$GADGET_DIR/functions/mass_storage.usb0" "$GADGET_DIR/configs/c.1/"
    fi

    # End functions
    ls /sys/class/udc > "$GADGET_DIR/UDC"
}

disable_gadget() {
    # https://github.com/larsks/systemd-usb-gadget/blob/master/remove-gadget.sh

    # echo "Removing strings from configurations"
    for dir in "$GADGET_DIR"/configs/*/strings/*; do
        [ -d "$dir" ] && rmdir "$dir"
    done

    # echo "Removing functions from configurations"
    for func in "$GADGET_DIR"/configs/*.*/*.*; do
        [ -e "$func" ] && rm "$func"
    done

    # echo "Removing configurations"
    for conf in "$GADGET_DIR"/configs/*; do
        [ -d "$conf" ] && rmdir "$conf"
    done

    # echo "Removing functions"
    for func in "$GADGET_DIR"/functions/*.*; do
        [ -d "$func" ] && rmdir "$func"
    done

    # echo "Removing strings"
    for str in "$GADGET_DIR"/strings/*; do
        [ -d "$str" ] && rmdir "$str"
    done

    # echo "Removing gadget"
    rmdir "$GADGET_DIR"

    modprobe -r --remove-holders -f usb_f_hid
    modprobe -r --remove-holders -f usb_f_mass_storage
    modprobe -r --remove-holders -f libcomposite
}

case "${1:-}" in
    on)
        if exists_gadget; then
            echo "Gadget already exists."
        else
            enable_gadget "${2:-}"
            echo "Gadget added."
        fi
        ;;
    off)
        if exists_gadget; then
            disable_gadget
            echo "Gadget removed."
        else
            echo "Gadget does not exist."
        fi
        ;;
    check)
        if exists_gadget; then
            exit 0
        else
            exit 1
        fi
        ;;
    *)
        echo "Usage: $0 {on|off|check} [FILE]"
        exit 1
        ;;
esac
