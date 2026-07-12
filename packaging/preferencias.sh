#!/usr/bin/env bash
# Abre as Preferências do EverVox (prefs da extensão GNOME) — é o Exec do
# lançador evervox.desktop. Se o GNOME Shell da sessão ainda não enxerga a
# extensão (no Wayland ele só varre os diretórios de extensões no login,
# então logo após instalar o pacote ela "não existe"), avisa que é preciso
# relogar em vez de falhar em silêncio.
#
# Uso: evervox-preferencias

EXT_UUID="evervox@evervox.local"

if gnome-extensions info "$EXT_UUID" >/dev/null 2>&1; then
    exec gnome-extensions prefs "$EXT_UUID"
fi

TITULO="EverVox"
MENSAGEM="Para abrir as Preferências, saia da sessão e entre de novo (logout/login): o GNOME só reconhece a extensão recém-instalada no próximo login. Depois, rode 'evervox-pos-instalar' se ainda não rodou."

if command -v notify-send >/dev/null 2>&1; then
    notify-send --app-name="$TITULO" "$TITULO" "$MENSAGEM"
elif command -v zenity >/dev/null 2>&1; then
    zenity --info --title="$TITULO" --text="$MENSAGEM"
else
    echo "$TITULO: $MENSAGEM" >&2
    exit 1
fi
