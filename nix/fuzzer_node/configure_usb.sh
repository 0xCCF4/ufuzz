#!/usr/bin/env bash

# https://www.kernel.org/doc/Documentation/usb/gadget_configfs.txt

exists_gadget() {
    [ -d thesis ]
}

enable_gadget() {
    local FILE=${1:-}

    # https://www.isticktoit.net/?p=1383

    mkdir -p thesis
    pushd thesis > /dev/null
    echo 0x1d6b > idVendor # Linux Foundation
    echo 0x0104 > idProduct # Multifunction Composite Gadget
    echo 0x0100 > bcdDevice # v1.0.0
    echo 0x0200 > bcdUSB # USB2
    mkdir -p strings/0x409
    echo "0000000000000124" > strings/0x409/serialnumber
    echo "Thesis" > strings/0x409/manufacturer
    echo "Thesis USB Device" > strings/0x409/product
    mkdir -p configs/c.1/strings/0x409
    echo "Standard config" > configs/c.1/strings/0x409/configuration
    echo 250 > configs/c.1/MaxPower
    # Add functions here

    # HID keyboard
    mkdir -p functions/hid.usb0
    echo 1 > functions/hid.usb0/protocol
    echo 1 > functions/hid.usb0/subclass
    echo 8 > functions/hid.usb0/report_length
    echo -ne \\x05\\x01\\x09\\x06\\xa1\\x01\\x05\\x07\\x19\\xe0\\x29\\xe7\\x15\\x00\\x25\\x01\\x75\\x01\\x95\\x08\\x81\\x02\\x95\\x01\\x75\\x08\\x81\\x03\\x95\\x05\\x75\\x01\\x05\\x08\\x19\\x01\\x29\\x05\\x91\\x02\\x95\\x01\\x75\\x03\\x91\\x03\\x95\\x06\\x75\\x08\\x15\\x00\\x25\\x65\\x05\\x07\\x19\\x00\\x29\\x65\\x81\\x00\\xc0 > functions/hid.usb0/report_desc
    ln -s functions/hid.usb0 configs/c.1/

    if [ -n "${FILE:-}" ]; then
        # Mass storage
        mkdir -p functions/mass_storage.usb0
        echo 1 > functions/mass_storage.usb0/stall
        echo 0 > functions/mass_storage.usb0/lun.0/cdrom
        echo 0 > functions/mass_storage.usb0/lun.0/ro
        echo 0 > functions/mass_storage.usb0/lun.0/nofua
        echo "$FILE" > functions/mass_storage.usb0/lun.0/file
        ln -s functions/mass_storage.usb0 configs/c.1/
    fi

    # End functions
    ls /sys/class/udc > UDC

    popd > /dev/null
}

disable_gadget() {
    pushd thesis > /dev/null

    echo "" > UDC

    rm -f configs/c.1/hid.usb0
    rm -f configs/c.1/mass_storage.usb0

    rm -f configs/c.1/strings/0x409/configuration

    rmdir -p configs/c.1/strings/0x409
    rmdir -p configs/c.1

    rmdir -p functions/hid.usb0
    rmdir -p functions/mass_storage.usb0

    popd > /dev/null
    rmdir thesis
}

pushd "/sys/kernel/config/usb_gadget/" > /dev/null


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
    *)
        echo "Usage: $0 {on|off} [FILE]"
        exit 1
        ;;
esac

popd > /dev/null