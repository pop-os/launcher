#!/usr/bin/sh
#
# name: Lock
# icon: locked
# description: Lock the system
# keywords: lock

set -eu

dbus-send --type=method_call --dest=org.gnome.ScreenSaver /org/gnome/ScreenSaver org.gnome.ScreenSaver.Lock
