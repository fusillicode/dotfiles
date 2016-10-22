#!/bin/sh

free_space=$(df -H | grep '/dev/disk1' | grep -o '[0-9]\+G' | tail -1)

echo "$free_space | size=12"
