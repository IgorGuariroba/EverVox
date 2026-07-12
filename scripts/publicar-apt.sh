#!/usr/bin/env bash
# Gera (ou regenera) o repositório APT plano do EverVox a partir dos .deb
# presentes no diretório — é o que o workflow de release publica no GitHub
# Pages para o `apt upgrade` dos usuários encontrar as versões novas sem
# baixar .deb na mão.
#
# Repositório "plano" (flat repo): Packages/Release/InRelease na raiz, sem
# dists/ nem pool/ — suficiente para um pacote só e aceito pelo apt com uma
# linha `deb [signed-by=...] <url> ./` no sources.list.
#
# Uso: publicar-apt.sh <dir-do-repo> [deb...]
#   <dir-do-repo>  diretório do repositório (ex.: o checkout do branch
#                  gh-pages + /apt); os .deb passados são copiados para lá
#                  antes de regenerar os índices.
#
# Assinatura: usa a chave GPG padrão do keyring corrente (no CI, importada
# do secret APT_GPG_PRIVATE_KEY antes de chamar este script). Sem chave
# secreta disponível, falha — repositório sem assinatura é recusado pelo
# apt moderno.
set -euo pipefail

if [ $# -lt 1 ]; then
    echo "uso: $0 <dir-do-repo> [deb...]" >&2
    exit 1
fi

DIR_REPO="$1"
shift

for ferramenta in dpkg-scanpackages apt-ftparchive gpg; do
    command -v "$ferramenta" >/dev/null 2>&1 || {
        echo "erro: '$ferramenta' não encontrado (instale dpkg-dev, apt-utils e gnupg)." >&2
        exit 1
    }
done

if ! gpg --list-secret-keys --with-colons 2>/dev/null | grep -q '^sec'; then
    echo "erro: nenhuma chave GPG secreta no keyring — sem ela o repositório sairia sem assinatura e o apt o recusaria." >&2
    exit 1
fi

mkdir -p "$DIR_REPO"
for deb in "$@"; do
    cp "$deb" "$DIR_REPO/"
done

cd "$DIR_REPO"

echo "==> Gerando índice Packages..."
# --multiversion mantém versões antigas listadas: dá para instalar uma
# versão específica com apt install evervox=<versão>.
dpkg-scanpackages --multiversion . >Packages
gzip -9 --keep --force Packages

echo "==> Gerando Release assinado..."
apt-ftparchive \
    -o APT::FTPArchive::Release::Origin=EverVox \
    -o APT::FTPArchive::Release::Label=EverVox \
    -o APT::FTPArchive::Release::Suite=estavel \
    -o APT::FTPArchive::Release::Architectures=amd64 \
    -o "APT::FTPArchive::Release::Description=Repositório APT do EverVox (ditado por voz para Ubuntu GNOME/Wayland)" \
    release . >Release
gpg --batch --yes --clearsign --output InRelease Release
gpg --batch --yes --armor --detach-sign --output Release.gpg Release

echo "==> Exportando a chave pública (evervox.gpg)..."
gpg --export >evervox.gpg

echo "==> Repositório pronto em $DIR_REPO:"
ls -1 "$DIR_REPO"
