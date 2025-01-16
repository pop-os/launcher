#!/usr/bin/env sh
#
# name: Power off
# icon: system-shutdown
# description: Shut down the system
# keywords: power off shutdown poweroff

set -eu

is_gnome() {
  command -v dbus-send >/dev/null && \
    command -v gnome-session-quit >/dev/null && \
    dbus-send --print-reply --dest=org.gnome.Shell /org/gnome/Shell org.freedesktop.DBus.Properties.Get string:org.gnome.Shell string:ShellVersion >/dev/null 2>&1
}

is_cosmic() {
  command -v cosmic-osd >/dev/null && [ "$XDG_SESSION_DESKTOP" = "COSMIC" ]
}

if is_cosmic; then
  cosmic-osd shutdown
elif is_gnome; then
  gnome-session-quit --power-off
elif command -v systemctl >/dev/null; then
  systemctl poweroff
fi
