#!/usr/bin/env bash
# E2E: pipeline de Ditado ponta a ponta em ambiente headless (issue #24).
#
# Três estágios, na linguagem da issue:
#   1. Sinais de estado (D-Bus) — obrigatório: qualquer falha aqui derruba o
#      teste. Sobe um barramento de sessão isolado, um PipeWire próprio com
#      microfone virtual, o Daemon real (Engine local, whisper base), executa
#      um Ditado com fala sintetizada e confere a sequência
#      Estado(gravando) → Estado(processando) → Estado(ocioso).
#   2. Entrega (clipboard) — degradável: precisa do sway headless; sem ele
#      o estágio é pulado com aviso claro e o teste segue valendo pelo 1.
#   3. Colar simulado (uinput) — degradável: precisa de /dev/uinput gravável
#      e leitura de /dev/input/event*; sem isso é pulado com aviso claro.
#
# Pré-requisitos de pacote (Ubuntu):
#   pipewire wireplumber pipewire-pulse pulseaudio-utils wl-clipboard
#   dbus-daemon libespeak-ng1 python3 curl
#   sway              (estágio 2; opcional — degrada se ausente)
#   acesso a uinput   (estágio 3; opcional — degrada se ausente)
#
# Uso:
#   ./scripts/e2e.sh
#
# Variáveis de ambiente:
#   EVERVOX_BIN_DIR        diretório com evervox e evervox-daemon
#                          (default: ./target/release; builda se ausentes)
#   EVERVOX_MODELO_CACHE   diretório com ggml-base.bin pré-baixado
#                          (default: ~/.cache/evervox-e2e)
#   EVERVOX_TIMEOUT_DAEMON segundos para o Daemon subir (default: 60)
#   EVERVOX_TIMEOUT_DITADO segundos para o Ditado processar (default: 30)
set -euo pipefail

log()   { echo "==> $*"; }
aviso() { echo "AVISO: $*" >&2; }
falha() { echo "FALHA: $*" >&2; exit 1; }

# Como `falha`, mas mostrando antes o fim do daemon.log — é lá que está o
# motivo real de um Toggle recusado (ex.: "microfone indisponível").
falha_com_log_do_daemon() {
    echo "--- daemon.log (últimas 50 linhas) ---"
    tail -50 "${DIR_TMP}/daemon.log" 2>/dev/null || true
    falha "$@"
}

# ── limpeza ──────────────────────────────────────────────────────────────────
# Todo processo em segundo plano entra em PIDS_LIMPEZA na ordem em que sobe;
# o trap os derruba na ordem inversa (Daemon antes do PipeWire, PipeWire
# antes do D-Bus) mesmo quando uma `falha` no meio do roteiro dá exit 1 —
# sem isso, um E2E que falha cedo deixaria dbus/pipewire/sway órfãos
# pendurando o job do CI.
PIDS_LIMPEZA=()
DIR_TMP="$(mktemp -d)"

limpar() {
    local i
    for ((i = ${#PIDS_LIMPEZA[@]} - 1; i >= 0; i--)); do
        kill "${PIDS_LIMPEZA[i]}" 2>/dev/null || true
    done
    for ((i = ${#PIDS_LIMPEZA[@]} - 1; i >= 0; i--)); do
        wait "${PIDS_LIMPEZA[i]}" 2>/dev/null || true
    done
    rm -rf "$DIR_TMP"
}
trap limpar EXIT

# ── helpers de espera ────────────────────────────────────────────────────────
# Espera até `timeout` segundos pela condição (os argumentos restantes,
# executados como comando). Retorna 1 se o tempo estourar — quem chama
# decide se isso é `falha` ou degradação.
esperar_condicao() {
    local timeout="$1"
    shift
    local i
    for i in $(seq 1 "$timeout"); do
        if "$@"; then return 0; fi
        sleep 1
    done
    return 1
}

# ── configuração ─────────────────────────────────────────────────────────────
DIR_REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN_DIR="${EVERVOX_BIN_DIR:-$DIR_REPO/target/release}"
MODELO_CACHE="${EVERVOX_MODELO_CACHE:-$HOME/.cache/evervox-e2e}"
TIMEOUT_DAEMON="${EVERVOX_TIMEOUT_DAEMON:-60}"
TIMEOUT_DITADO="${EVERVOX_TIMEOUT_DITADO:-30}"

export XDG_RUNTIME_DIR="${DIR_TMP}/runtime"
export XDG_CONFIG_HOME="${DIR_TMP}/config"
export XDG_DATA_HOME="${DIR_TMP}/data"
export XDG_CACHE_HOME="${DIR_TMP}/cache"
mkdir -p "$XDG_RUNTIME_DIR" "$XDG_CONFIG_HOME" "$XDG_DATA_HOME" "$XDG_CACHE_HOME"
# O D-Bus exige runtime dir acessível só pelo dono (o default do umask deixa
# o grupo escrever e o dbus-daemon reclama).
chmod 700 "$XDG_RUNTIME_DIR"

ERROS=0
ESTAGIO2_OK=0
ESTAGIO3_OK=0

# ── infraestrutura do estágio 1 (obrigatória) ────────────────────────────────

subir_dbus() {
    log "Iniciando D-Bus session bus isolado..."
    DBUS_SESSION_BUS_ADDRESS="unix:path=${XDG_RUNTIME_DIR}/bus"
    export DBUS_SESSION_BUS_ADDRESS
    dbus-daemon --session --address="$DBUS_SESSION_BUS_ADDRESS" --nofork --nopidfile &
    PIDS_LIMPEZA+=($!)
    esperar_condicao 5 test -S "${XDG_RUNTIME_DIR}/bus" ||
        falha "socket do D-Bus não apareceu em ${XDG_RUNTIME_DIR}/bus"
}

pipewire_pronto() {
    pactl info >/dev/null 2>&1
}

subir_pipewire() {
    log "Iniciando PipeWire + WirePlumber isolados..."
    pipewire &
    PIDS_LIMPEZA+=($!)
    pipewire-pulse &
    PIDS_LIMPEZA+=($!)
    wireplumber &
    PIDS_LIMPEZA+=($!)
    esperar_condicao 10 pipewire_pronto ||
        falha "PipeWire não respondeu via pactl em 10s"
}

mic_virtual_visivel() {
    pactl list sinks short 2>/dev/null | grep -q evervox_mic
}

criar_mic_virtual() {
    log "Criando microfone virtual (null sink → monitor source)..."
    pactl load-module module-null-sink \
        sink_name=evervox_mic \
        sink_properties=device.description=EverVox_E2E_Mic >/dev/null
    esperar_condicao 5 mic_virtual_visivel ||
        falha "sink 'evervox_mic' não apareceu em 'pactl list sinks short'"

    # Tudo que for tocado no sink evervox_mic aparece no monitor source; com
    # ele como default, o Daemon o captura como se fosse o microfone real.
    esperar_condicao 5 bash -c "pactl list sources short 2>/dev/null | grep -q 'evervox_mic.monitor'" ||
        falha "monitor source 'evervox_mic.monitor' não apareceu"
    pactl set-default-source evervox_mic.monitor
    log "Microfone virtual pronto (source default: evervox_mic.monitor)"
}

garantir_binarios() {
    if [ -x "$BIN_DIR/evervox" ] && [ -x "$BIN_DIR/evervox-daemon" ]; then
        log "Binários já existem em $BIN_DIR, pulando build."
    else
        log "Compilando binários (release)..."
        (cd "$DIR_REPO" && cargo build --release --bin evervox --bin evervox-daemon)
    fi
}

preparar_config() {
    log "Criando config.toml (Engine local, Limpeza desligada)..."
    mkdir -p "$XDG_CONFIG_HOME/evervox"
    cat >"$XDG_CONFIG_HOME/evervox/config.toml" <<'EOF'
idioma = "pt"
modelo_local = "base"
engine = "local"

[limpeza]
habilitada = false
EOF
}

garantir_modelo() {
    local modelo_dir="${XDG_DATA_HOME}/evervox/modelos"
    local modelo="${modelo_dir}/ggml-base.bin"
    mkdir -p "$modelo_dir"

    if [ -f "${MODELO_CACHE}/ggml-base.bin" ]; then
        log "Copiando modelo whisper base do cache ($MODELO_CACHE)..."
        cp "${MODELO_CACHE}/ggml-base.bin" "$modelo"
    else
        log "Baixando modelo whisper base (~140 MB)..."
        curl -L --progress-bar -o "$modelo" \
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin"
        mkdir -p "$MODELO_CACHE"
        cp "$modelo" "${MODELO_CACHE}/ggml-base.bin"
    fi
}

daemon_pronto() {
    # Sonda pelo log, não por `evervox status`: o status também consulta o
    # GNOME Keyring, e no barramento isolado a ativação D-Bus de
    # org.freedesktop.secrets pode pendurar a chamada indefinidamente.
    grep -q "pronto em" "${DIR_TMP}/daemon.log" 2>/dev/null
}

subir_daemon() {
    log "Iniciando evervox-daemon (timeout ${TIMEOUT_DAEMON}s)..."
    "$BIN_DIR/evervox-daemon" >"${DIR_TMP}/daemon.log" 2>&1 &
    PIDS_LIMPEZA+=($!)
    if ! esperar_condicao "$TIMEOUT_DAEMON" daemon_pronto; then
        echo "--- daemon.log (últimas 50 linhas) ---"
        tail -50 "${DIR_TMP}/daemon.log" 2>/dev/null || true
        falha "Daemon não subiu em ${TIMEOUT_DAEMON}s"
    fi
    log "Daemon pronto."
}

# ── estágio 2 (degradável): compositor Wayland para a Entrega ───────────────
# Sway (wlroots), não Weston: o wl-clipboard depende do protocolo
# zwlr_data_control_v1 para operar sem foco de janela — Weston não o
# implementa, e num compositor headless sem teclado o wl-copy nunca obtém o
# serial de input que o caminho sem data-control exige (falha com exit 1).

encontrar_socket_wayland() {
    SOCKET_WAYLAND="$(find "$XDG_RUNTIME_DIR" -maxdepth 1 -name 'wayland-*' -type s -printf '%f\n' 2>/dev/null | head -1)"
    [ -n "$SOCKET_WAYLAND" ]
}

subir_wayland() {
    if ! command -v sway >/dev/null 2>&1; then
        aviso "Estágio 2 (Entrega/clipboard) pulado: 'sway' não instalado."
        return
    fi

    log "Iniciando sway headless..."
    # Config mínima: só os defaults do sway, nenhum atalho ou autostart.
    : >"${DIR_TMP}/sway.conf"
    WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_RENDERER=pixman \
        sway --config "${DIR_TMP}/sway.conf" >"${DIR_TMP}/sway.log" 2>&1 &
    PIDS_LIMPEZA+=($!)

    if ! esperar_condicao 10 encontrar_socket_wayland; then
        aviso "Estágio 2 (Entrega/clipboard) pulado: sway não criou socket wayland em 10s."
        return
    fi
    export WAYLAND_DISPLAY="$SOCKET_WAYLAND"
    ESTAGIO2_OK=1
    log "Compositor Wayland pronto (display: $WAYLAND_DISPLAY)"
}

# ── estágio 3 (degradável): leitura dos eventos do teclado virtual ──────────

iniciar_leitor_de_teclas() {
    if [ ! -w /dev/uinput ]; then
        aviso "Estágio 3 (colar simulado/uinput) pulado: /dev/uinput sem permissão de escrita."
        return
    fi

    # O Daemon cria o teclado virtual "evervox-colar-simulado" na
    # inicialização; o leitor encontra o /dev/input/event* correspondente e
    # registra cada tecla pressionada/solta em teclas.log.
    log "Iniciando leitor de eventos do teclado virtual..."
    python3 "$DIR_REPO/scripts/e2e-teclas.py" \
        >"${DIR_TMP}/teclas.log" 2>"${DIR_TMP}/teclas.err" &
    LEITOR_PID=$!
    PIDS_LIMPEZA+=("$LEITOR_PID")

    # O leitor sai imediatamente (com stderr explicando) se não achar o
    # dispositivo ou não tiver permissão de leitura — nesses casos o estágio
    # degrada em vez de falhar o teste.
    sleep 2
    if kill -0 "$LEITOR_PID" 2>/dev/null; then
        ESTAGIO3_OK=1
    else
        aviso "Estágio 3 (colar simulado/uinput) pulado: $(cat "${DIR_TMP}/teclas.err" 2>/dev/null || echo 'leitor de eventos encerrou')"
    fi
}

# Conteúdo semeado no clipboard antes do Ditado: quando o colar simulado
# funciona, a Entrega restaura o clipboard anterior no final (ADR 0001) — a
# sentinela é como o estágio 2 verifica essa restauração.
SENTINELA_CLIPBOARD="clipboard-anterior-do-e2e"

preparar_clipboard_estagio2() {
    [ "$ESTAGIO2_OK" = 1 ] || return 0
    log "Semeando o clipboard com a sentinela e monitorando mudanças..."
    printf '%s' "$SENTINELA_CLIPBOARD" | wl-copy
    # Cada mudança de clipboard (a cópia da Transcrição e a restauração da
    # sentinela) é registrada — é aqui que a Transcrição fica observável,
    # já que o clipboard final volta ao estado anterior.
    wl-paste --watch tee -a "${DIR_TMP}/clipboard_mudancas.log" >/dev/null 2>&1 &
    PIDS_LIMPEZA+=($!)
    sleep 0.5
}

# ── o Ditado ─────────────────────────────────────────────────────────────────

executar_ditado() {
    log "Monitorando sinais D-Bus de estado..."
    dbus-monitor --address "$DBUS_SESSION_BUS_ADDRESS" \
        "type='signal',interface='com.evervox.Daemon1',member='Estado'" \
        >"${DIR_TMP}/estados.log" 2>/dev/null &
    PIDS_LIMPEZA+=($!)
    sleep 0.5

    log "Gerando fixture de fala ('Ditado de teste automatizado')..."
    python3 "$DIR_REPO/scripts/e2e-fixture.py" "${DIR_TMP}/ditado.wav"

    log "Toggle 1 (iniciar gravação)..."
    local estado
    estado="$("$BIN_DIR/evervox" toggle 2>/dev/null)"
    [ "$estado" = "gravando" ] ||
        falha_com_log_do_daemon "Toggle 1 deveria retornar 'gravando', retornou '$estado'"

    sleep 0.3 # margem para o microfone abrir o stream de captura
    log "Tocando o fixture no microfone virtual..."
    # paplay/pw-play/pw-cat bloqueiam até o áudio terminar de tocar.
    paplay --device=evervox_mic "${DIR_TMP}/ditado.wav" 2>/dev/null ||
        pw-play --target=evervox_mic "${DIR_TMP}/ditado.wav" 2>/dev/null ||
        pw-cat --playback --target=evervox_mic "${DIR_TMP}/ditado.wav" 2>/dev/null ||
        falha "não foi possível tocar o fixture (paplay/pw-play/pw-cat)"
    sleep 0.5 # margem para as últimas amostras atravessarem o grafo

    log "Toggle 2 (encerrar e processar)..."
    estado="$("$BIN_DIR/evervox" toggle 2>/dev/null)"
    [ "$estado" = "ocioso" ] ||
        falha_com_log_do_daemon "Toggle 2 deveria retornar 'ocioso', retornou '$estado'"

    log "Aguardando o Processando terminar (timeout ${TIMEOUT_DITADO}s)..."
    if ! esperar_condicao "$TIMEOUT_DITADO" grep -q '"ocioso"' "${DIR_TMP}/estados.log"; then
        aviso "Processando não sinalizou 'ocioso' em ${TIMEOUT_DITADO}s — conferindo mesmo assim..."
    fi
    sleep 1 # margem para clipboard/eventos assentarem
}

# ── asserções ────────────────────────────────────────────────────────────────

verificar_estado() {
    local estado="$1"
    if grep -q "\"$estado\"" "${DIR_TMP}/estados.log" 2>/dev/null; then
        log "  ✓ Estado('$estado') detectado"
    else
        aviso "  ✗ Estado('$estado') NÃO detectado"
        ERROS=$((ERROS + 1))
    fi
}

verificar_estagio1_estados() {
    log "Estágio 1 — sequência de estados D-Bus:"
    verificar_estado gravando
    verificar_estado processando
    verificar_estado ocioso
}

verificar_estagio2_clipboard() {
    if [ "$ESTAGIO2_OK" != 1 ]; then
        log "Estágio 2 — pulado (sem compositor Wayland)."
        return
    fi

    log "Estágio 2 — Transcrição via clipboard e restauração:"

    # A Transcrição passa pelo clipboard durante a Entrega e é observada no
    # log de mudanças. O whisper base transcreve a fala sintetizada de forma
    # imprecisa (ex.: "E até o teste automático." para "Ditado de teste
    # automatizado"), então o assert é difuso: alguma palavra-chave precisa
    # aparecer ("autom" cobre automatizado/automático); match exato flakaria.
    if grep -qiE 'teste|autom|ditado' "${DIR_TMP}/clipboard_mudancas.log" 2>/dev/null; then
        log "  ✓ Transcrição passou pelo clipboard"
    else
        aviso "  ✗ Transcrição NÃO passou pelo clipboard"
        echo "--- clipboard_mudancas.log ---"
        cat "${DIR_TMP}/clipboard_mudancas.log" 2>/dev/null || true
        ERROS=$((ERROS + 1))
    fi

    # O estado final do clipboard depende do colar simulado (ADR 0001):
    # colar funcionou → o clipboard anterior (a sentinela) é restaurado;
    # colar indisponível → a Transcrição permanece como fallback manual.
    local texto_final
    texto_final="$(wl-paste --no-newline 2>/dev/null || echo '')"
    if [ "$ESTAGIO3_OK" = 1 ]; then
        if [ "$texto_final" = "$SENTINELA_CLIPBOARD" ]; then
            log "  ✓ Clipboard anterior (sentinela) restaurado após a Entrega"
        else
            aviso "  ✗ Clipboard deveria ter voltado à sentinela, contém: '$texto_final'"
            ERROS=$((ERROS + 1))
        fi
    else
        if echo "$texto_final" | grep -qiE 'teste|autom|ditado'; then
            log "  ✓ Transcrição ficou no clipboard (fallback sem colar simulado)"
        else
            aviso "  ✗ Clipboard deveria conter a Transcrição (fallback), contém: '$texto_final'"
            ERROS=$((ERROS + 1))
        fi
    fi
}

verificar_estagio3_uinput() {
    if [ "$ESTAGIO3_OK" != 1 ]; then
        log "Estágio 3 — pulado (sem acesso a uinput)."
        return
    fi

    log "Estágio 3 — atalho de colar no teclado virtual:"
    # Sem a extensão GNOME neste ambiente, o Foco degrada para o atalho
    # padrão: esperamos Ctrl+V (e não Ctrl+Shift+V de terminal).
    if grep -q "KEY_LEFTCTRL press" "${DIR_TMP}/teclas.log" 2>/dev/null &&
        grep -q "KEY_V press" "${DIR_TMP}/teclas.log" 2>/dev/null; then
        log "  ✓ Ctrl+V simulado detectado nos eventos de uinput"
    else
        aviso "  ✗ Ctrl+V NÃO detectado nos eventos de uinput"
        echo "--- teclas.log ---"
        cat "${DIR_TMP}/teclas.log" 2>/dev/null || true
        ERROS=$((ERROS + 1))
    fi
}

# ── roteiro ──────────────────────────────────────────────────────────────────

subir_dbus
subir_pipewire
criar_mic_virtual
subir_wayland
garantir_binarios
preparar_config
garantir_modelo
subir_daemon
iniciar_leitor_de_teclas
preparar_clipboard_estagio2
executar_ditado

verificar_estagio1_estados
verificar_estagio2_clipboard
verificar_estagio3_uinput

if [ "$ERROS" -gt 0 ]; then
    echo ""
    echo "--- daemon.log (últimas 60 linhas) ---"
    tail -60 "${DIR_TMP}/daemon.log" 2>/dev/null || true
    falha "E2E falhou com $ERROS erro(s)"
fi

log "E2E concluído com sucesso."
