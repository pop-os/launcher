#!/usr/bin/env sh
#
# name: Log Out
# icon: system-log-out
# description: Log out to the login screen
# keywords: log out logout

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
  cosmic-osd log-out
elif is_gnome; then
  gnome-session-quit --logout
elif command -v loginctl >/dev/null; then
  loginctl terminate-user "${USER}"
fi
