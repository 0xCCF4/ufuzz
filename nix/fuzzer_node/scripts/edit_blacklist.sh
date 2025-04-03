#!/usr/bin/env bash

sudo mount ~/disk.img /mnt
sudo nano /mnt/blacklist.txt
sudo umount /mnt
sync