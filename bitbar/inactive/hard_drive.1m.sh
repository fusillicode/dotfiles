#!/usr/bin/env bash

free_space=$(df -H | /opt/homebrew/bin/rg '/dev/disk1' | /opt/homebrew/bin/rg -o '[0-9]\+G' | tail -1)
echo "$free_space | size=10 color=white"
