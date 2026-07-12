#!/usr/bin/env bash
# Gera o pacote .deb do EverVox com cargo-deb.
#
# O pacote é definido no crate daemon ([package.metadata.deb] em
# crates/daemon/Cargo.toml), mas inclui também o binário `evervox` do crate
# cli — por isso o build do workspace acontece aqui, antes do cargo-deb
# (que roda com --no-build).
#
# Uso: ./scripts/empacotar-deb.sh
# Requisitos: toolchain Rust e `cargo install cargo-deb`.
set -euo pipefail

DIR_REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if ! command -v cargo-deb >/dev/null 2>&1; then
    echo "erro: cargo-deb não encontrado — rode 'cargo install cargo-deb' antes." >&2
    exit 1
fi

cd "$DIR_REPO"
echo "==> Compilando os binários (release)..."
cargo build --release --bin evervox --bin evervox-daemon

echo "==> Gerando o .deb..."
cargo deb -p evervox-daemon --no-build

echo
echo "==> Pacote gerado em target/debian/. Instale com:"
ls -1 "$DIR_REPO"/target/debian/*.deb | tail -1 | sed 's/^/    sudo apt install /'
