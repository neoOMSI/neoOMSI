#!/usr/bin/env bash
set -euo pipefail

for i in 1 2 3; do
  sudo apt-get update -qq && sudo apt-get install -y -qq --no-install-recommends \
    pkg-config \
    libasound2-dev \
    libudev-dev \
    libgtk-3-dev \
    libxkbcommon-dev \
    libwayland-dev \
    libssl-dev \
    zip \
    clang \
    lld && break
  sleep 20
done
