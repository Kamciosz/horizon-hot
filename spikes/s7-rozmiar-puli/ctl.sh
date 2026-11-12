#!/bin/bash
# PL: horizon-ctl podłączony do klienta uruchomionego przez MCP `play` (jego socket nie jest domyślny).
# EN: horizon-ctl attached to the client started by MCP `play` (its socket is not the default one).
source "$(dirname "$0")/env.sh"
exec "${CTL[@]}" "$@"
