#!/usr/bin/env python3
"""Gera o áudio fixture do E2E (ver scripts/e2e.sh e issue #24): fala em
pt-br sintetizada via libespeak-ng, chamada direto pela biblioteca (não
pelo binário `espeak-ng`, que nem sempre está instalado como dependência
transitiva de pacotes de acessibilidade) — só precisa de `libespeak-ng1`.

Uso: e2e-fixture.py <caminho-do-wav-de-saida>
"""
import ctypes
import struct
import sys
import wave

FRASE = "Ditado de teste automatizado"
AUDIO_OUTPUT_SYNCHRONOUS = 2
ESPEAK_CHARS_UTF8 = 1

CALLBACK = ctypes.CFUNCTYPE(
    ctypes.c_int, ctypes.POINTER(ctypes.c_short), ctypes.c_int, ctypes.c_void_p
)


def gerar(caminho_saida: str) -> None:
    lib = ctypes.CDLL("libespeak-ng.so.1")
    taxa_amostragem = lib.espeak_Initialize(AUDIO_OUTPUT_SYNCHRONOUS, 0, None, 0)
    if taxa_amostragem <= 0:
        raise RuntimeError("espeak_Initialize falhou (libespeak-ng1 instalada?)")

    amostras = bytearray()

    def callback(wav, numsamples, _events):
        if numsamples > 0 and wav:
            bloco = ctypes.cast(wav, ctypes.POINTER(ctypes.c_short * numsamples)).contents
            amostras.extend(struct.pack(f"<{numsamples}h", *bloco))
        return 0

    cb = CALLBACK(callback)
    lib.espeak_SetSynthCallback(cb)
    lib.espeak_SetVoiceByName(b"pt-br")

    texto = FRASE.encode("utf-8")
    lib.espeak_Synth(texto, len(texto) + 1, 0, 0, 0, ESPEAK_CHARS_UTF8, None, None)
    lib.espeak_Synchronize()

    with wave.open(caminho_saida, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(taxa_amostragem)
        w.writeframes(bytes(amostras))


if __name__ == "__main__":
    if len(sys.argv) != 2:
        print("uso: e2e-fixture.py <caminho-do-wav-de-saida>", file=sys.stderr)
        sys.exit(1)
    gerar(sys.argv[1])
    print(f"fixture gerado em {sys.argv[1]} ('{FRASE}')")
