#!/usr/bin/env bash
# E2E: pipeline de Ditado ponta a ponta em ambiente headless.
#
# Sobe um ambiente mínimo (D-Bus session, PipeWire + WirePlumber, Weston
# headless), builda o projeto, inicia o Daemon com Engine local (whisper.cpp
# modelo base), gera áudio de fala sintetizada, simula um Ditado completo e
# confere que a Transcrição chegou ao clipboard.
#
# Pré-requisitos de pacote (Ubuntu):
#   pipewire wireplumber pipewire-pulse weston wl-clipboard dbus-daemon
#   libespeak-ng1 python3 curl cmake libclang-dev pkg-config libasound2-dev
#
# Uso:
#   ./scripts/e2e.sh                  # usa ./target/release/
#   EVERVOX_BIN_DIR=./bin ./scripts/e2e.sh  # binários em outro lugar
#
# Variáveis de ambiente:
#   EVERVOX_BIN_DIR        diretório com evervox e evervox-daemon
#                          (default: ./target/release)
#   EVERVOX_MODELO_CACHE   diretório com ggml-base.bin pré-baixado
#                          (default: ~/.cache/evervox-e2e)
#   EVERVOX_TIMEOUT_DAEMON segundos para esperar o Daemon subir (default: 60)
#   EVERVOX_TIMEOUT_DITADO segundos para esperar o Ditado processar (default: 30)
set -euo pipefail

# ── helpers ────────────────────────────────────────────────────────────────
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

passo()  { echo -e "${GREEN}==>${NC} $*"; }
falha()  { echo -e "${RED}FALHA:${NC} $*" >&2; exit 1; }
aviso()  { echo -e "${YELLOW}AVISO:${NC} $*" >&2; }
esperar_arquivo() {
    local arquivo="$1" timeout="${2:-10}" msg="${3:-}"
    for _ in $(seq 1 "$timeout"); do
        if [ -e "$arquivo" ]; then return 0; fi
        sleep 1
    done
    falha "timeout esperando ${msg:-$arquivo} ($timeout s)"
}

# ── ambiente isolado ───────────────────────────────────────────────────────
DIR_REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN_DIR="${EVERVOX_BIN_DIR:-$DIR_REPO/target/release}"
MODELO_CACHE="${EVERVOX_MODELO_CACHE:-$HOME/.cache/evervox-e2e}"
TIMEOUT_DAEMON="${EVERVOX_TIMEOUT_DAEMON:-60}"
TIMEOUT_DITADO="${EVERVOX_TIMEOUT_DITADO:-30}"

DIR_TMP="$(mktemp -d)"
trap 'rm -rf "$DIR_TMP"' EXIT

export XDG_RUNTIME_DIR="${DIR_TMP}/runtime"
export XDG_CONFIG_HOME="${DIR_TMP}/config"
export XDG_DATA_HOME="${DIR_TMP}/data"
export XDG_CACHE_HOME="${DIR_TMP}/cache"
mkdir -p "$XDG_RUNTIME_DIR" "$XDG_CONFIG_HOME" "$XDG_DATA_HOME" "$XDG_CACHE_HOME"

# ── 1. D-Bus session bus ───────────────────────────────────────────────────
passo "Iniciando D-Bus session bus..."
DBUS_SESSION_BUS_ADDRESS="unix:path=${XDG_RUNTIME_DIR}/bus"
export DBUS_SESSION_BUS_ADDRESS

dbus-daemon --session --address="$DBUS_SESSION_BUS_ADDRESS" --nofork --nopidfile &
DBUS_PID=$!
esperar_arquivo "${XDG_RUNTIME_DIR}/bus" 5 "socket do D-Bus"

# ── 2. PipeWire + WirePlumber ──────────────────────────────────────────────
passo "Iniciando PipeWire..."
pipewire &
PIPEWIRE_PID=$!
sleep 1

pipewire-pulse &
PIPEWIRE_PULSE_PID=$!
sleep 1

wireplumber &
WIREPLUMBER_PID=$!
sleep 2

# ── 3. Microfone virtual (null sink → monitor source) ──────────────────────
passo "Criando microfone virtual..."
pactl load-module module-null-sink \
    sink_name=evervox_mic \
    sink_properties=device.description=EverVox_E2E_Mic

# O monitor source do null sink é o que o Daemon vai capturar como "microfone".
# Tudo que for tocado no sink evervox_mic aparece nesse source.
SINK_ID="$(pactl list short sinks | grep evervox_mic | awk '{print $1}')"
MONITOR_SOURCE="$(pactl list short sources | grep "${SINK_ID}\.monitor" | awk '{print $2}')"
pactl set-default-source "$MONITOR_SOURCE"
passo "Microfone virtual: sink=$SINK_ID, source=$MONITOR_SOURCE (default)"

# ── 4. Weston headless (wl-copy/wl-paste precisam de compositor) ───────────
passo "Iniciando Weston headless..."
weston --backend=headless-backend.so --socket=wayland-0 &
WESTON_PID=$!
sleep 2

# O socket do Weston pode estar em XDG_RUNTIME_DIR ou em um subdiretório
if [ -S "${XDG_RUNTIME_DIR}/wayland-0" ]; then
    export WAYLAND_DISPLAY=wayland-0
elif [ -S "${XDG_RUNTIME_DIR}/wayland-1" ]; then
    export WAYLAND_DISPLAY=wayland-1
else
    # Weston pode ter criado o socket em outro lugar; procuramos
    WAYLAND_SOCKET="$(find "$XDG_RUNTIME_DIR" -name 'wayland-*' -type s 2>/dev/null | head -1)"
    if [ -n "$WAYLAND_SOCKET" ]; then
        export WAYLAND_DISPLAY="$(basename "$WAYLAND_SOCKET")"
    else
        falha "Weston não criou socket wayland em $XDG_RUNTIME_DIR"
    fi
fi
passo "Wayland display: $WAYLAND_DISPLAY"

# ── 5. Build ───────────────────────────────────────────────────────────────
if [ -x "$BIN_DIR/evervox" ] && [ -x "$BIN_DIR/evervox-daemon" ]; then
    passo "Binários já existem em $BIN_DIR, pulando build."
else
    passo "Compilando binários (release)..."
    (cd "$DIR_REPO" && cargo build --release --bin evervox --bin evervox-daemon)
fi

# ── 6. Config do Daemon ────────────────────────────────────────────────────
passo "Criando config.toml..."
mkdir -p "$XDG_CONFIG_HOME/evervox"
cat > "$XDG_CONFIG_HOME/evervox/config.toml" <<'EOF'
idioma = "pt"
modelo_local = "base"
engine = "local"

[limpeza]
habilitada = false
EOF

# ── 7. Modelo whisper ──────────────────────────────────────────────────────
MODELO_DIR="${XDG_DATA_HOME}/evervox/modelos"
MODELO="${MODELO_DIR}/ggml-base.bin"
mkdir -p "$MODELO_DIR"

if [ -f "$MODELO" ]; then
    passo "Modelo whisper base já existe em $MODELO."
elif [ -f "${MODELO_CACHE}/ggml-base.bin" ]; then
    passo "Copiando modelo do cache ($MODELO_CACHE)..."
    cp "${MODELO_CACHE}/ggml-base.bin" "$MODELO"
else
    passo "Baixando modelo whisper base (~140 MB)..."
    curl -L --progress-bar \
        -o "$MODELO" \
        "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin"
    # Espelha no cache para a próxima execução
    mkdir -p "$MODELO_CACHE"
    cp "$MODELO" "${MODELO_CACHE}/ggml-base.bin"
fi

# ── 8. Iniciar Daemon ──────────────────────────────────────────────────────
passo "Iniciando evervox-daemon..."
"$BIN_DIR/evervox-daemon" >"${DIR_TMP}/daemon.log" 2>&1 &
DAEMON_PID=$!

passo "Aguardando Daemon ficar pronto (timeout ${TIMEOUT_DAEMON}s)..."
for i in $(seq 1 "$TIMEOUT_DAEMON"); do
    if "$BIN_DIR/evervox" status 2>/dev/null | grep -q "ativo"; then
        passo "Daemon pronto após ${i}s."
        break
    fi
    if [ "$i" -eq "$TIMEOUT_DAEMON" ]; then
        echo "--- daemon.log (últimas 50 linhas) ---"
        tail -50 "${DIR_TMP}/daemon.log" 2>/dev/null || true
        falha "Daemon não subiu em ${TIMEOUT_DAEMON}s"
    fi
    sleep 1
done

# ── 9. Gerar fixture de áudio ──────────────────────────────────────────────
passo "Gerando fixture de áudio..."
FIXTURE="${DIR_TMP}/ditado.wav"
python3 "$DIR_REPO/scripts/e2e-fixture.py" "$FIXTURE"
passo "Fixture: $(du -h "$FIXTURE" | cut -f1) — 'Ditado de teste automatizado'"

# ── 10. Monitorar sinais D-Bus de estado ───────────────────────────────────
dbus-monitor --address "$DBUS_SESSION_BUS_ADDRESS" \
    "type='signal',interface='com.evervox.Daemon1',member='Estado'" \
    >"${DIR_TMP}/estados.log" 2>/dev/null &
DBUS_MONITOR_PID=$!
sleep 0.5

# ── 11. Executar o Ditado ──────────────────────────────────────────────────
passo "Iniciando Toggle (gravando)..."
ESTADO_INICIAL="$("$BIN_DIR/evervox" toggle 2>/dev/null)"
echo "  Toggle 1 → $ESTADO_INICIAL"

if [ "$ESTADO_INICIAL" != "gravando" ]; then
    falha "Toggle 1 deveria retornar 'gravando', retornou '$ESTADO_INICIAL'"
fi

# Pequena pausa para o microfone abrir, depois toca o áudio no sink virtual
sleep 0.3
passo "Tocando fixture no microfone virtual..."
paplay --device="evervox_mic" "$FIXTURE" 2>/dev/null || \
    pw-play --target="evervox_mic" "$FIXTURE" 2>/dev/null || \
    pw-cat --playback --target="evervox_mic" "$FIXTURE" 2>/dev/null || \
    aviso "Não foi possível tocar o áudio (paplay/pw-play/pw-cat); tentando ffplay..."
# Dá tempo para o áudio terminar de tocar + um pouco de margem
sleep 3

passo "Encerrando Toggle (processando)..."
ESTADO_FINAL="$("$BIN_DIR/evervox" toggle 2>/dev/null)"
echo "  Toggle 2 → $ESTADO_FINAL"

if [ "$ESTADO_FINAL" != "ocioso" ]; then
    falha "Toggle 2 deveria retornar 'ocioso', retornou '$ESTADO_FINAL'"
fi

# ── 12. Aguardar o Processando ─────────────────────────────────────────────
passo "Aguardando Processando terminar (timeout ${TIMEOUT_DITADO}s)..."
for i in $(seq 1 "$TIMEOUT_DITADO"); do
    # O estado volta a "ocioso" no Overlay quando o Processando termina
    if grep -q '"ocioso"' "${DIR_TMP}/estados.log" 2>/dev/null; then
        passo "Processando concluído após ${i}s."
        break
    fi
    if [ "$i" -eq "$TIMEOUT_DITADO" ]; then
        aviso "Timeout do Processando (${TIMEOUT_DITADO}s) — conferindo mesmo assim..."
    fi
    sleep 1
done

# Pequena pausa extra para o clipboard ser atualizado
sleep 1

# ── 13. Parar monitor D-Bus ────────────────────────────────────────────────
kill "$DBUS_MONITOR_PID" 2>/dev/null || true
wait "$DBUS_MONITOR_PID" 2>/dev/null || true

# ── 14. Coletar evidências ─────────────────────────────────────────────────
passo "Coletando evidências..."

TEXTO_CLIPBOARD="$(wl-paste --no-newline 2>/dev/null || echo '')"
echo "  Clipboard: '$TEXTO_CLIPBOARD'"

ESTADOS="$(cat "${DIR_TMP}/estados.log" 2>/dev/null || echo '')"
echo "  Sinais D-Bus:"
echo "$ESTADOS" | sed 's/^/    /'

# ── 15. Asserções ──────────────────────────────────────────────────────────
ERROS=0

passo "Verificando sequência de estados D-Bus..."
if echo "$ESTADOS" | grep -q '"gravando"'; then
    passo "  ✓ 'gravando' detectado"
else
    aviso "  ✗ 'gravando' NÃO detectado"
    ERROS=$((ERROS + 1))
fi

if echo "$ESTADOS" | grep -q '"processando"'; then
    passo "  ✓ 'processando' detectado"
else
    aviso "  ✗ 'processando' NÃO detectado"
    ERROS=$((ERROS + 1))
fi

if echo "$ESTADOS" | grep -q '"ocioso"'; then
    passo "  ✓ 'ocioso' detectado"
else
    aviso "  ✗ 'ocioso' NÃO detectado"
    ERROS=$((ERROS + 1))
fi

passo "Verificando transcrição no clipboard..."
# O whisper base com áudio sintetizado em pt-br para "Ditado de teste
# automatizado" deve produzir algo razoável. Aceitamos qualquer texto
# não vazio — a qualidade da transcrição depende do modelo e do áudio.
if [ -n "$TEXTO_CLIPBOARD" ]; then
    passo "  ✓ Clipboard contém: '$TEXTO_CLIPBOARD'"
else
    aviso "  ✗ Clipboard vazio"
    ERROS=$((ERROS + 1))
fi

# ── 16. Logs em caso de falha ──────────────────────────────────────────────
if [ "$ERROS" -gt 0 ]; then
    echo ""
    echo "--- daemon.log (últimas 60 linhas) ---"
    tail -60 "${DIR_TMP}/daemon.log" 2>/dev/null || true
fi

# ── 17. Cleanup ────────────────────────────────────────────────────────────
passo "Encerrando processos..."
kill "$DAEMON_PID" 2>/dev/null || true
wait "$DAEMON_PID" 2>/dev/null || true
kill "$WESTON_PID" 2>/dev/null || true
wait "$WESTON_PID" 2>/dev/null || true
kill "$WIREPLUMBER_PID" 2>/dev/null || true
wait "$WIREPLUMBER_PID" 2>/dev/null || true
kill "$PIPEWIRE_PULSE_PID" 2>/dev/null || true
wait "$PIPEWIRE_PULSE_PID" 2>/dev/null || true
kill "$PIPEWIRE_PID" 2>/dev/null || true
wait "$PIPEWIRE_PID" 2>/dev/null || true
kill "$DBUS_PID" 2>/dev/null || true
wait "$DBUS_PID" 2>/dev/null || true

# ── 18. Resultado ──────────────────────────────────────────────────────────
echo ""
if [ "$ERROS" -eq 0 ]; then
    passo "E2E concluído com sucesso!"
    exit 0
else
    falha "E2E falhou com $ERROS erro(s)"
fi
