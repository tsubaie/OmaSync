#!/bin/bash
# Runs the multi-computer end-to-end tests in a container (see Dockerfile), with this checkout
# mounted. Build caches live in Docker volumes, so later runs only rebuild what changed.
set -euo pipefail
cd "$(dirname "$0")/../.."
docker build -t omasync-e2e -f e2e/multi/Dockerfile .
docker run --rm --hostname omarchy \
  --user "$(id -u):$(id -g)" -e HOME=/tmp/home \
  -v "$PWD":/app \
  -v omasync-e2e-target:/cache/target \
  omasync-e2e bash e2e/multi/in-container.sh
