#!/usr/bin/env bash
# Instalação de ponta a ponta do EverVox num Ubuntu GNOME/Wayland "limpo"
# (ver issue #10 e CONTEXT.md): builda os binários, registra o Daemon como
# serviço systemd --user, instala a extensão GNOME, configura as permissões
# de uinput e registra o atalho de teclado do Toggle.
#
# Uso: ./scripts/instalar.sh
#
# Variáveis de ambiente opcionais:
#   EVERVOX_ATALHO — combinação de teclas do Toggle (default: <Control><Alt>d)
#                    pode ser trocada depois em Configurações > Teclado >
#                    Atalhos personalizados.
set -euo pipefail

DIR_REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN_DIR="$HOME/.local/bin"
EXT_UUID="evervox@evervox.local"
EXT_DIR="$HOME/.local/share/gnome-shell/extensions/$EXT_UUID"
SYSTEMD_DIR="$HOME/.config/systemd/user"
ATALHO="${EVERVOX_ATALHO:-<Control><Alt>d}"
PRECISA_RELOGAR=0

log() { echo "==> $*"; }
aviso() { echo "AVISO: $*" >&2; }

verificar_pre_requisitos() {
    if ! command -v cargo >/dev/null 2>&1; then
        echo "erro: cargo não encontrado no PATH — instale o toolchain Rust antes de continuar." >&2
        exit 1
    fi
}

instalar_binarios() {
    log "Compilando os binários (release)..."
    (cd "$DIR_REPO" && cargo build --release --bin evervox --bin evervox-daemon)
    mkdir -p "$BIN_DIR"
    cp "$DIR_REPO/target/release/evervox" "$BIN_DIR/evervox"
    cp "$DIR_REPO/target/release/evervox-daemon" "$BIN_DIR/evervox-daemon"
    log "Binários instalados em $BIN_DIR"
    case ":$PATH:" in
        *":$BIN_DIR:"*) ;;
        *)
            aviso "$BIN_DIR não está no PATH. Adicione a linha abaixo ao seu ~/.bashrc (ou ~/.profile) e relogue:"
            # A linha é impressa literal (sem expandir), para o usuário colar no .bashrc.
            # shellcheck disable=SC2016
            echo '    export PATH="$HOME/.local/bin:$PATH"'
            ;;
    esac
}

instalar_extensao() {
    log "Instalando a extensão GNOME..."
    rm -rf "$EXT_DIR"
    mkdir -p "$EXT_DIR"
    cp "$DIR_REPO"/gnome-extension/*.js "$DIR_REPO"/gnome-extension/*.json "$DIR_REPO"/gnome-extension/*.css "$EXT_DIR"/
    if command -v gnome-extensions >/dev/null 2>&1; then
        if ! gnome-extensions enable "$EXT_UUID" 2>/dev/null; then
            aviso "não foi possível habilitar a extensão automaticamente; habilite em Extensões do GNOME."
            PRECISA_RELOGAR=1
        fi
    else
        aviso "'gnome-extensions' não encontrado; habilite a extensão manualmente depois de relogar."
        PRECISA_RELOGAR=1
    fi
    log "Extensão instalada em $EXT_DIR"
}

instalar_lancador() {
    log "Instalando o lançador (.desktop) e o ícone..."
    local apps_dir="$HOME/.local/share/applications"
    local icones_dir="$HOME/.local/share/icons/hicolor/scalable/apps"
    mkdir -p "$apps_dir" "$icones_dir"
    cp "$DIR_REPO/packaging/evervox.desktop" "$apps_dir/evervox.desktop"
    cp "$DIR_REPO/packaging/evervox.svg" "$icones_dir/evervox.svg"
    if command -v update-desktop-database >/dev/null 2>&1; then
        update-desktop-database "$apps_dir" 2>/dev/null || true
    fi
    log "Lançador instalado: EverVox aparece em 'Mostrar aplicativos' e abre as Preferências."
}

instalar_servico_systemd() {
    log "Registrando o serviço systemd --user..."
    mkdir -p "$SYSTEMD_DIR"
    cat >"$SYSTEMD_DIR/evervox.service" <<EOF
[Unit]
Description=EverVox - Daemon de ditado por voz
After=graphical-session.target

[Service]
ExecStart=$BIN_DIR/evervox-daemon
Restart=on-failure
RestartSec=2

[Install]
WantedBy=graphical-session.target
EOF
    systemctl --user daemon-reload
    systemctl --user enable --now evervox.service
    log "Serviço evervox habilitado e iniciado (systemctl --user status evervox)"
}

configurar_uinput() {
    log "Configurando permissões de uinput..."
    local regra="/etc/udev/rules.d/99-evervox-uinput.rules"
    if [ ! -f "$regra" ]; then
        echo 'KERNEL=="uinput", GROUP="input", MODE="0660"' | sudo tee "$regra" >/dev/null
        sudo udevadm control --reload-rules
        sudo udevadm trigger
        log "Regra udev de /dev/uinput instalada."
    else
        log "Regra udev de /dev/uinput já presente."
    fi

    if id -nG "$USER" | grep -qw input; then
        log "Usuário '$USER' já está no grupo 'input'."
    else
        sudo usermod -aG input "$USER"
        aviso "usuário adicionado ao grupo 'input': é preciso RELOGAR a sessão para o colar automático funcionar."
        PRECISA_RELOGAR=1
    fi
}

registrar_atalho() {
    if ! command -v gsettings >/dev/null 2>&1; then
        aviso "'gsettings' não encontrado; registre o atalho manualmente em Configurações > Teclado, apontando para '$BIN_DIR/evervox toggle'."
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
    gsettings set "$base_atalho" command "$BIN_DIR/evervox toggle"
    gsettings set "$base_atalho" binding "$ATALHO"
    log "Atalho registrado: $ATALHO -> evervox toggle"
}

verificar_pre_requisitos
instalar_binarios
instalar_extensao
instalar_lancador
instalar_servico_systemd
configurar_uinput
registrar_atalho

echo
log "Instalação concluída."
log "Rode 'evervox status' para conferir a saúde do serviço."
if [ "$PRECISA_RELOGAR" = "1" ]; then
    aviso "relogue a sessão (Wayland) antes de ditar — alguma etapa acima exige isso."
fi
