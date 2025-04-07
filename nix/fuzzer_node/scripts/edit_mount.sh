#!/usr/bin/env bash

PATH=$PATH:/run/current-system/sw/bin/

sudo mount ~/disk.img /mnt
pushd /mnt > /dev/null
echo "===== exit to finish editing ====="
sudo bash
echo "===== finished editing ====="
popd > /dev/null
sudo umount /mnt
sync