# Daemon escrito em Rust

O ecossistema de STT/ML tem bindings de primeira classe em Python (faster-whisper, VAD), o que faria a v1 mais rápida de desenvolver. Escolhemos Rust mesmo assim: o EverVox é um daemon residente de longa duração, e queremos binário único, RAM mínima fora do modelo e robustez sem runtime — aceitando o atrito maior nas bibliotecas de ML.

## Consequences

- STT local via `whisper-rs` (bindings do whisper.cpp); a máquina alvo não tem GPU dedicada (Intel Iris Xe, 8 threads), então os modelos viáveis são `base`/`small`, possivelmente com backend Vulkan.
- Áudio via `cpal`, D-Bus via `zbus`, clipboard via `wl-clipboard`/`arboard`, APIs cloud via `reqwest`.
