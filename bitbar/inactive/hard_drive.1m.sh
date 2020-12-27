#!/bin/bash

free_space=$(df -H | /usr/local/bin/rg '/dev/disk1' | /usr/local/bin/rg -o '[0-9]\+G' | tail -1)
echo "$free_space | size=10 color=white"
