#!/usr/bin/env bash

case "${1:-}" in
    send_keys)
        send_keys "${2:-}" "${3:-}" "${4:-}" "${5:-}"
        ;;
    power_button)
        power_button "${2:-}" "${3:-}" "${4:-}" "${5:-}"
        ;;
    skip_bios)
        skip_bios "${2:-}" "${3:-}" "${4:-}" "${5:-}"
        ;;
    configure_usb)
        configure_usb "${2:-}" "${3:-}" "${4:-}" "${5:-}"
        ;;
    *)
        echo "Invalid argument: $1"
        echo "Usage: manage {send_keys|power_button|skip_bios|configure_usb} [...]"

        echo -n " "
        send_keys

        echo -n " "
        power_button

        echo " skipbios"

        echo -n " "
        configure_usb

        exit 1
        ;;
esac
