#!/bin/sh

OLDIFS=$IFS

get_cpu_stats() {
    local IFS=$'\n'
    topdata=($(top -F -R -l2 -o cpu -n 5 -s 2 -stats pid,command,cpu))
    nlines=${#topdata[@]}

    IFS=$OLDIFS
    for ((i = nlines / 2; i < nlines; i++)); do
        line=(${topdata[$i]})
        word=${line[0]}
        if [ "$word" = PID ]; then
            top_5=("${topdata[@]:$i}")
        fi
    done

    IFS=$'\n'
}

get_cpu_stats
echo "${top_5[1]} | size=12"

echo "---"

top_5=("${top_5[@]/%/ | font=Menlo}")
IFS=$'\n'
echo "${top_5[*]}"
