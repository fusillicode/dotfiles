#!/bin/sh

# Stop containers
docker stop $(docker ps -a -q)
# Remove all containers with volumes
docker rm -v $(docker ps -a -q)
# Remove all images
docker rmi -f $(docker images -q)
