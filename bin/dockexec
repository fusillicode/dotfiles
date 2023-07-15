#!/bin/bash

container_name=${1:-"fast-data-dev"}
command=${2:-bash}

docker exec -it "$(docker ps | /opt/homebrew/bin/rg "$container_name" | awk '{print $1;}')" "$command"
