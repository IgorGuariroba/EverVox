#!/usr/bin/env bash
# Abre as Preferências do EverVox — é o Exec do lançador evervox.desktop.
# Roda o app standalone (`aplicativo_preferencias.js`, issue #47) direto do
# diretório da extensão via gjs, sem pedir nada ao GNOME Shell: no Wayland o
# Shell só varre os diretórios de extensões no login, então logo após
# instalar ou atualizar o pacote o `gnome-extensions prefs` falharia — mas
# os arquivos já estão no disco e a UI funciona igual. O caminho pelo Shell
# fica como fallback para quando o gjs não estiver disponível.
#
# Uso: evervox-preferencias

EXT_UUID="evervox@evervox.local"
TITULO="EverVox"

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

# Instalação de fonte (~/.local) tem precedência sobre a do pacote, como nos
# diretórios de extensões do próprio GNOME.
for dir in "$HOME/.local/share/gnome-shell/extensions/$EXT_UUID" \
    "/usr/share/gnome-shell/extensions/$EXT_UUID"; do
    app="$dir/aplicativo_preferencias.js"
    if [ -f "$app" ] && command -v gjs >/dev/null 2>&1; then
        exec gjs -m "$app"
    fi
done

# Sem gjs (ou pacote antigo sem o app standalone): tenta pelo Shell.
if gnome-extensions info "$EXT_UUID" >/dev/null 2>&1; then
    exec gnome-extensions prefs "$EXT_UUID"
fi

if [ -d "$HOME/.local/share/gnome-shell/extensions/$EXT_UUID" ] ||
    [ -d "/usr/share/gnome-shell/extensions/$EXT_UUID" ]; then
    avisar "Para abrir as Preferências instale o 'gjs' (sudo apt install gjs) ou saia e entre de novo na sessão (logout/login) para o GNOME carregar a extensão."
else
    avisar "A extensão GNOME do EverVox não está instalada. Reinstale o pacote (sudo apt install --reinstall evervox) e rode 'evervox-pos-instalar'."
fi
exit 1
