#!/usr/bin/env python3
"""Lê os eventos do teclado virtual da Entrega para o estágio 3 do E2E (ver
scripts/e2e.sh e issue #24): encontra o dispositivo de input criado pelo
Daemon ("evervox-colar-simulado", ver EntregaClipboard em
crates/daemon/src/entrega.rs), lê os eventos crus de /dev/input/event* e
imprime uma linha por tecla ("KEY_LEFTCTRL press", "KEY_V release", ...),
para o script assertar que o atalho de colar foi de fato simulado.

Lê o dispositivo direto (stdlib apenas) em vez de exigir `evtest`/python-evdev
instalados no runner. Sai com código != 0 e explicação no stderr quando o
dispositivo não existe ou a leitura não é permitida — o e2e.sh trata esses
casos como degradação do estágio, não como falha do teste.
"""
import glob
import struct
import sys
import time

NOME_DISPOSITIVO = "evervox-colar-simulado"
# struct input_event em 64 bits: timeval (2x long) + type + code + value.
FORMATO_EVENTO = "llHHi"
TAMANHO_EVENTO = struct.calcsize(FORMATO_EVENTO)
EV_KEY = 1
TECLAS = {29: "KEY_LEFTCTRL", 42: "KEY_LEFTSHIFT", 47: "KEY_V"}
ACOES = {0: "release", 1: "press", 2: "repeat"}


def achar_dispositivo(timeout_s: float) -> str | None:
    """Procura em sysfs o event device cujo nome é o do teclado da Entrega."""
    fim = time.monotonic() + timeout_s
    while time.monotonic() < fim:
        for caminho in glob.glob("/sys/class/input/event*/device/name"):
            try:
                with open(caminho) as f:
                    nome = f.read().strip()
            except OSError:
                continue
            if nome == NOME_DISPOSITIVO:
                evento = caminho.split("/")[4]  # "eventN"
                return f"/dev/input/{evento}"
        time.sleep(0.2)
    return None


def main() -> int:
    dispositivo = achar_dispositivo(timeout_s=5)
    if dispositivo is None:
        print(
            f"dispositivo '{NOME_DISPOSITIVO}' não encontrado em /dev/input",
            file=sys.stderr,
        )
        return 3

    try:
        f = open(dispositivo, "rb")
    except PermissionError:
        print(f"sem permissão de leitura em {dispositivo}", file=sys.stderr)
        return 2

    with f:
        while True:
            dados = f.read(TAMANHO_EVENTO)
            if len(dados) < TAMANHO_EVENTO:
                return 0  # dispositivo removido (Daemon encerrou)
            _, _, tipo, codigo, valor = struct.unpack(FORMATO_EVENTO, dados)
            if tipo != EV_KEY:
                continue
            tecla = TECLAS.get(codigo, f"KEY_{codigo}")
            acao = ACOES.get(valor, str(valor))
            print(f"{tecla} {acao}", flush=True)


if __name__ == "__main__":
    sys.exit(main())
