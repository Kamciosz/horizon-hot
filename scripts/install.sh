#!/bin/sh
# PL: Instalacja aplikacji gracza hot-play na macOS i Linuksie (M11.3, ADR 0016):
#   curl -fsSL https://github.com/Kamciosz/horizon-hot/releases/download/hot-play/install.sh | sh -s -- ADRES
# Pobiera plik z wydania `hot-play`, sprawdza SHA-256, zapisuje adres serwera i robi skrót „Graj”.
# Ponowne uruchomienie jest bezpieczne: plik trafia na miejsce dopiero po sprawdzeniu sumy.
# EN: Installs the hot-play player app on macOS and Linux (M11.3, ADR 0016). Downloads the file
# from the `hot-play` release, checks SHA-256, saves the server address and makes a "Graj"
# shortcut. Running it again is safe: the file is moved in only after the checksum matches.
set -eu

BASE=${HOT_PLAY_BASE:-https://github.com/Kamciosz/horizon-hot/releases/download/hot-play}
ADDRESS=${1:-}

fail() {
    echo "Błąd: $*" >&2
    exit 1
}

case "$(uname -s)-$(uname -m)" in
    Darwin-arm64) ASSET=hot-play-macos-arm64 ;;
    Darwin-x86_64) ASSET=hot-play-macos-x64 ;;
    Linux-x86_64) ASSET=hot-play-linux-x64 ;;
    *) fail "hot-play nie ma pliku dla systemu $(uname -s) $(uname -m)." ;;
esac

if [ "$(uname -s)" = Darwin ]; then
    DATA=${HOT_PLAY_HOME:-$HOME/Library/Application Support/hot-play}
else
    DATA=${HOT_PLAY_HOME:-${XDG_DATA_HOME:-$HOME/.local/share}/hot-play}
fi
BIN=$DATA/hot-play

download() {
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL --retry 3 -A hot-play -o "$2" "$1"
    elif command -v wget >/dev/null 2>&1; then
        wget -q -U hot-play -O "$2" "$1"
    else
        fail "brak curl i wget; zainstaluj jeden z nich."
    fi
}

sha256() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d' ' -f1
    else
        shasum -a 256 "$1" | cut -d' ' -f1
    fi
}

mkdir -p "$DATA"
TMP=$DATA/.install
rm -rf "$TMP"
mkdir -p "$TMP"
trap 'rm -rf "$TMP"' EXIT INT TERM

echo "Pobieram hot-play ($ASSET)…"
download "$BASE/SHA256SUMS" "$TMP/SHA256SUMS" || fail "nie pobrałem $BASE/SHA256SUMS. Sprawdź internet."
download "$BASE/$ASSET" "$TMP/$ASSET" || fail "nie pobrałem $BASE/$ASSET. Sprawdź internet."
WANT=$(awk -v f="$ASSET" '$2 == f || $2 == "*" f { print $1 }' "$TMP/SHA256SUMS")
[ -n "$WANT" ] || fail "SHA256SUMS nie zna pliku $ASSET."
GOT=$(sha256 "$TMP/$ASSET")
[ "$GOT" = "$WANT" ] || fail "pobrany plik ma inną sumę SHA-256 niż w wydaniu (przerwane pobieranie?). Uruchom instalację jeszcze raz."
chmod 755 "$TMP/$ASSET"
mv -f "$TMP/$ASSET" "$BIN"
"$BIN" --version >/dev/null || fail "pobrany hot-play się nie uruchamia."
echo "Zainstalowany: $BIN"

if [ -n "$ADDRESS" ]; then
    HOT_PLAY_NO_UPDATE=1 "$BIN" ustaw "$ADDRESS"
fi

# PL: Skrót „Graj”: menu hot-play w oknie terminala. EN: The "Graj" shortcut: the hot-play menu
# in a terminal window.
if [ "$(uname -s)" = Darwin ]; then
    DESKTOP=$HOME/Desktop
    if [ -d "$DESKTOP" ]; then
        printf '#!/bin/sh\nexec "%s"\n' "$BIN" >"$DESKTOP/Graj.command"
        chmod 755 "$DESKTOP/Graj.command"
        echo "Skrót: $DESKTOP/Graj.command (dwuklik)."
    fi
else
    APPS=${XDG_DATA_HOME:-$HOME/.local/share}/applications
    mkdir -p "$APPS"
    ENTRY=$APPS/hot-play.desktop
    printf '[Desktop Entry]\nType=Application\nName=Graj\nComment=Minecraft Horizon na serwerze horizon-hot\nExec="%s"\nTerminal=true\nCategories=Game;\n' "$BIN" >"$ENTRY"
    chmod 755 "$ENTRY"
    DESKTOP=$(xdg-user-dir DESKTOP 2>/dev/null || echo "$HOME/Desktop")
    if [ -d "$DESKTOP" ]; then
        cp "$ENTRY" "$DESKTOP/Graj.desktop"
        chmod 755 "$DESKTOP/Graj.desktop"
        # PL: GNOME uruchamia skrót z pulpitu dopiero po oznaczeniu go jako zaufany.
        # EN: GNOME runs a desktop shortcut only once it is marked trusted.
        gio set "$DESKTOP/Graj.desktop" metadata::trusted true 2>/dev/null || true
        echo "Skrót: $DESKTOP/Graj.desktop (dwuklik) i Graj w menu aplikacji."
    else
        echo "Skrót: Graj w menu aplikacji."
    fi
fi

echo "Gotowe. Kliknij dwa razy skrót Graj albo uruchom: \"$BIN\""
