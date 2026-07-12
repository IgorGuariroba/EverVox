#!/usr/bin/env bash
# Passos por usuário depois de instalar o pacote .deb do EverVox: o dpkg roda
# como root e não sabe qual usuário vai ditar, então grupo `input`, extensão
# GNOME, serviço systemd --user e atalho de teclado ficam a cargo deste
# comando, rodado pelo próprio usuário na sessão gráfica.
#
# Uso: evervox-pos-instalar
#
# Variáveis de ambiente opcionais:
#   EVERVOX_ATALHO — combinação de teclas do Toggle (default: <Control><Alt>d)
#                    pode ser trocada depois em Configurações > Teclado >
#                    Atalhos personalizados.
set -euo pipefail

EXT_UUID="evervox@evervox.local"
ATALHO="${EVERVOX_ATALHO:-<Control><Alt>d}"
URL_REPO_APT="https://igorguariroba.github.io/EverVox/apt"
PRECISA_RELOGAR=0

log() { echo "==> $*"; }
aviso() { echo "AVISO: $*" >&2; }

entrar_no_grupo_input() {
    if id -nG "$USER" | grep -qw input; then
        log "Usuário '$USER' já está no grupo 'input'."
    else
        log "Adicionando '$USER' ao grupo 'input' (pede sudo)..."
        sudo usermod -aG input "$USER"
        aviso "usuário adicionado ao grupo 'input': é preciso RELOGAR a sessão para o colar automático funcionar."
        PRECISA_RELOGAR=1
    fi
}

habilitar_extensao() {
    if command -v gnome-extensions >/dev/null 2>&1; then
        if ! gnome-extensions enable "$EXT_UUID" 2>/dev/null; then
            aviso "não foi possível habilitar a extensão automaticamente; habilite em Extensões do GNOME após relogar."
            PRECISA_RELOGAR=1
        else
            log "Extensão GNOME habilitada."
        fi
    else
        aviso "'gnome-extensions' não encontrado; habilite a extensão manualmente depois de relogar."
        PRECISA_RELOGAR=1
    fi
}

habilitar_servico() {
    log "Habilitando o serviço systemd --user..."
    systemctl --user daemon-reload
    systemctl --user enable --now evervox.service
    log "Serviço evervox habilitado e iniciado (systemctl --user status evervox)"
}

registrar_atalho() {
    if ! command -v gsettings >/dev/null 2>&1; then
        aviso "'gsettings' não encontrado; registre o atalho manualmente em Configurações > Teclado, apontando para 'evervox toggle'."
        return
    fi

    log "Registrando o atalho de teclado do Toggle ($ATALHO)..."
    local caminho_atalho="/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/evervox/"
    local base="org.gnome.settings-daemon.plugins.media-keys"
    local existentes
    existentes="$(gsettings get "$base" custom-keybindings)"

    if [[ "$existentes" != *"$caminho_atalho"* ]]; then
        if [ "$existentes" = "@as []" ] || [ "$existentes" = "[]" ]; then
            gsettings set "$base" custom-keybindings "['$caminho_atalho']"
        else
            local sem_colchete_final="${existentes%]}"
            gsettings set "$base" custom-keybindings "${sem_colchete_final}, '$caminho_atalho']"
        fi
    fi

    local base_atalho="org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:$caminho_atalho"
    gsettings set "$base_atalho" name "EverVox Toggle"
    gsettings set "$base_atalho" command "/usr/bin/evervox toggle"
    gsettings set "$base_atalho" binding "$ATALHO"
    log "Atalho registrado: $ATALHO -> evervox toggle"
}

# Cadastra o repositório APT do projeto para as próximas versões chegarem
# num 'sudo apt upgrade' comum, sem baixar .deb na mão. Sem rede (ou antes
# de o repositório existir), só avisa — nada aqui é pré-requisito do ditado.
configurar_repositorio_apt() {
    local keyring="/usr/share/keyrings/evervox.gpg"
    local lista="/etc/apt/sources.list.d/evervox.list"

    if [ -f "$lista" ] && [ -f "$keyring" ]; then
        log "Repositório APT do EverVox já configurado (updates via 'sudo apt upgrade')."
        return
    fi

    log "Configurando o repositório APT do EverVox (pede sudo)..."
    local chave_tmp
    chave_tmp="$(mktemp)"
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$URL_REPO_APT/evervox.gpg" -o "$chave_tmp" || true
    elif command -v wget >/dev/null 2>&1; then
        wget -qO "$chave_tmp" "$URL_REPO_APT/evervox.gpg" || true
    fi
    if ! [ -s "$chave_tmp" ]; then
        rm -f "$chave_tmp"
        aviso "não consegui baixar a chave do repositório APT ($URL_REPO_APT/evervox.gpg); atualizações automáticas ficam para a próxima vez que rodar o evervox-pos-instalar."
        return
    fi

    sudo install -m 644 "$chave_tmp" "$keyring"
    rm -f "$chave_tmp"
    echo "deb [signed-by=$keyring] $URL_REPO_APT ./" | sudo tee "$lista" >/dev/null
    log "Repositório APT configurado: as próximas versões chegam pelo 'sudo apt upgrade'."
}

entrar_no_grupo_input
habilitar_extensao
habilitar_servico
registrar_atalho
configurar_repositorio_apt

echo
log "Configuração do usuário concluída."
log "Rode 'evervox status' para conferir a saúde do serviço."
if [ "$PRECISA_RELOGAR" = "1" ]; then
    aviso "relogue a sessão (Wayland) antes de ditar — alguma etapa acima exige isso."
fi
