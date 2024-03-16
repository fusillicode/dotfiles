#!/usr/bin/env bash

# shellcheck disable=SC2207
cpu_burner_data=($(ps -Ar -o pid= -o %cpu= -o comm= | head -n 1))

cpu_burner_name=${cpu_burner_data[2]##*/}

echo "${cpu_burner_data[0]}  $cpu_burner_name  ${cpu_burner_data[1]}% | size=12"
