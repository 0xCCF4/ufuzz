#!/usr/bin/env bash

PATH=$PATH:/run/current-system/sw/bin/

echo "$@"

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
    init)
        power_button release
        if [ -f /home/thesis/disk.img ]; then
            if ! configure_usb check; then
                configure_usb on /home/thesis/disk.img
            fi
        fi
        ;;
    check)
        curl 127.0.0.1:8000/alive
        ;;
    *)
        echo "$0 {send_keys|power_button|skip_bios|configure_usb|check|init} [...]"

        echo -n "     "
        send_keys || true

        echo -n "     "
        power_button || true

        echo "     skipbios"

        echo -n "     "
        configure_usb || true

        echo "     check"

        exit 1
        ;;
esac
