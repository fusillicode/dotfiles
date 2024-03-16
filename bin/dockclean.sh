#!/usr/bin/env bash

docker rm "$(docker ps -aq)"
docker volume rm "$(docker volume ls --format '{{.Name}}')"
