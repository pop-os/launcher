#!/usr/bin/env sh
#
# name: Suspend
# icon: system-suspend
# description: Suspend the system
# keywords: suspend sleep

set -eu

systemctl suspend
