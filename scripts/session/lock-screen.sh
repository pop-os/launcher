#!/usr/bin/sh
#
# name: Lock
# icon: locked
# description: Lock the system
# keywords: lock

set -eu

loginctl lock-session
