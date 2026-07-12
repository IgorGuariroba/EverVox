#!/usr/bin/env bash
# Abre as Preferências do EverVox (prefs da extensão GNOME) — é o Exec do
# lançador evervox.desktop. No Wayland o GNOME Shell só varre os diretórios
# de extensões no login, então logo após instalar OU ATUALIZAR o pacote a
# extensão "não existe" para a sessão atual. Nesse caso, em vez de só avisar,
# oferece encerrar a sessão na hora (gnome-session-quit) — único jeito de o
# Shell passar a enxergar a extensão. Se os arquivos nem estão no disco, o
# problema é outro: instalação incompleta.
#
# Uso: evervox-preferencias

EXT_UUID="evervox@evervox.local"
TITULO="EverVox"

if gnome-extensions info "$EXT_UUID" >/dev/null 2>&1; then
    exec gnome-extensions prefs "$EXT_UUID"
fi

avisar() {
    local mensagem="$1"
    if command -v notify-send >/dev/null 2>&1; then
        notify-send --app-name="$TITULO" "$TITULO" "$mensagem"
    elif command -v zenity >/dev/null 2>&1; then
        zenity --info --title="$TITULO" --text="$mensagem"
    else
        echo "$TITULO: $mensagem" >&2
    fi
}

extensao_no_disco() {
    [ -f "/usr/share/gnome-shell/extensions/$EXT_UUID/metadata.json" ] ||
        [ -f "$HOME/.local/share/gnome-shell/extensions/$EXT_UUID/metadata.json" ]
}

if ! extensao_no_disco; then
    avisar "A extensão GNOME do EverVox não está instalada. Reinstale o pacote (sudo apt install --reinstall evervox) e rode 'evervox-pos-instalar'."
    exit 1
fi

# Arquivos no disco, mas o Shell da sessão não os vê: o pacote foi instalado
# ou atualizado nesta sessão. Oferece o logout na hora; o gnome-session-quit
# ainda mostra a confirmação do próprio GNOME antes de encerrar.
if command -v zenity >/dev/null 2>&1; then
    if zenity --question --title="$TITULO" \
        --text="O EverVox foi instalado ou atualizado nesta sessão, e o GNOME só carrega a extensão no próximo login.\n\nEncerrar a sessão agora?" \
        --ok-label="Encerrar sessão" --cancel-label="Agora não"; then
        exec gnome-session-quit --logout
    fi
    exit 0
fi

avisar "O EverVox foi instalado ou atualizado nesta sessão: saia e entre de novo (logout/login) para o GNOME carregar a extensão."
exit 1
