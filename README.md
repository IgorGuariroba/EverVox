# EverVox

Serviço de ditado por voz para Ubuntu (GNOME/Wayland): um atalho de teclado
captura a fala em qualquer app, transcreve, limpa o texto via LLM (opcional)
e entrega ao campo de texto focado. Inspirado no FluidVoice (macOS).

Vocabulário do domínio (Ditado, Toggle, Engine, Limpeza, Entrega, Overlay
etc.) está documentado em [`CONTEXT.md`](CONTEXT.md) — vale ler antes de
mexer no código.

## Requisitos

- Ubuntu com GNOME Shell em Wayland (alvo da v1; outros compositores/DEs não
  são suportados).
- `sudo` disponível (para a regra udev de `/dev/uinput`).
- Só para instalar a partir do código-fonte: [toolchain
  Rust](https://rustup.rs) (`cargo` no PATH).

## Instalação pelo pacote .deb

Baixe o `evervox_*.deb` da [página de
releases](https://github.com/IgorGuariroba/EverVox/releases) (publicado
automaticamente quando a versão dos crates muda na `main`, ver
`.github/workflows/release.yml`) — ou gere você mesmo, ver abaixo — e
instale:

```bash
sudo apt install ./evervox_0.1.0-1_amd64.deb
evervox-pos-instalar
```

O pacote instala os binários em `/usr/bin`, a extensão GNOME em
`/usr/share/gnome-shell/extensions`, o serviço `systemd --user` em
`/usr/lib/systemd/user`, a regra udev de `/dev/uinput` e um lançador
("EverVox" em Mostrar aplicativos, que abre as Preferências) — e o
`evervox-pos-instalar` conclui os passos que dependem do seu usuário
(grupo `input`, habilitar extensão e serviço, atalho de teclado; aceita a
variável `EVERVOX_ATALHO` como no instalador de fonte). Se ele avisar que é
preciso **relogar a sessão**, faça isso antes de ditar.

Para gerar o pacote a partir do código-fonte (requer o toolchain Rust e
`cargo install cargo-deb`):

```bash
./scripts/empacotar-deb.sh
```

## Instalação a partir do código-fonte

```bash
git clone https://github.com/IgorGuariroba/EverVox.git
cd EverVox
./scripts/instalar.sh
```

O script:

1. Builda `evervox` (CLI) e `evervox-daemon` (Daemon) em modo release e os
   copia para `~/.local/bin`.
2. Instala a extensão GNOME em
   `~/.local/share/gnome-shell/extensions/evervox@evervox.local` e tenta
   habilitá-la.
3. Registra e habilita o Daemon como serviço `systemd --user`
   (`~/.config/systemd/user/evervox.service`), com restart automático em
   caso de crash.
4. Instala a regra udev que libera `/dev/uinput` para o grupo `input` e
   adiciona seu usuário a esse grupo, se ainda não estiver.
5. Registra um atalho de teclado do GNOME apontando para `evervox toggle`
   (default `<Control><Alt>d`; troque com a variável `EVERVOX_ATALHO` antes
   de rodar o script, ou depois em Configurações > Teclado > Atalhos
   personalizados).

Se o script avisar que é preciso **relogar a sessão** (grupo `input` novo,
ou a extensão não pôde ser habilitada automaticamente), faça isso antes de
ditar — no Wayland não há como recarregar essas permissões sem uma nova
sessão.

Depois de instalado, confira a saúde do serviço:

```bash
evervox status
```

## Primeiros passos

- O Engine e a Limpeza padrão são `local` (whisper.cpp, sem rede) e
  desligada, respectivamente — dá para ditar sem nenhuma chave de API.
- Para usar o Engine `cloud` (OpenAI) ou ligar a Limpeza (OpenAI/Anthropic),
  edite `~/.config/evervox/config.toml` (criado com os defaults na primeira
  execução do Daemon) e salve a chave do provedor escolhido:

  ```bash
  evervox set-key openai
  evervox set-key anthropic
  ```

  A chave fica só no GNOME Keyring — nunca em config, log ou variável de
  ambiente.
- Acione o atalho registrado para começar a gravar; acione de novo para
  encerrar e disparar a transcrição. O Overlay da extensão GNOME mostra
  "gravando" e "processando"; um beep marca início e fim da gravação.
- Todos os campos de `config.toml` (idioma, modelo local, Limpeza,
  Vocabulário, terminais conhecidos) estão documentados em `CONTEXT.md`.

## Desinstalação

Instalação via `.deb`:

```bash
systemctl --user disable --now evervox.service
sudo apt remove evervox
# Opcional — config, dados e chaves: veja os passos "Opcional" abaixo.
```

Instalação a partir do código-fonte:

```bash
systemctl --user disable --now evervox.service
rm -f ~/.config/systemd/user/evervox.service
systemctl --user daemon-reload

gnome-extensions disable evervox@evervox.local 2>/dev/null || true
rm -rf ~/.local/share/gnome-shell/extensions/evervox@evervox.local

rm -f ~/.local/bin/evervox ~/.local/bin/evervox-daemon

# Atalho de teclado: remova "EverVox Toggle" em Configurações > Teclado >
# Atalhos personalizados (ou via gsettings, revertendo a chave
# custom-keybindings de org.gnome.settings-daemon.plugins.media-keys).

# Opcional — config e dados (modelo baixado):
rm -rf ~/.config/evervox ~/.local/share/evervox

# Opcional — chaves de API salvas no GNOME Keyring: apague as entradas do
# serviço "evervox" pelo app "Senhas e Chaves" (seahorse), ou:
secret-tool clear service evervox username openai
secret-tool clear service evervox username anthropic

# Opcional — regra udev de /dev/uinput (só remova se nenhum outro app do
# sistema depender do grupo 'input' criado por ela):
sudo rm -f /etc/udev/rules.d/99-evervox-uinput.rules
sudo udevadm control --reload-rules
```

## Desenvolvimento

Após clonar, ative os git hooks versionados (fmt/clippy no commit, testes no
push):

```bash
git config core.hooksPath .githooks
```

```bash
cargo test --workspace   # testes
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
```

Push direto para `main` é bloqueado; todo merge entra por PR com o check
`ci` verde.

### Release

Para publicar uma versão nova, suba a versão nos `Cargo.toml` dos crates
num PR e mescle: o merge na `main` dispara o workflow `Release`, que cria
a tag `v<versão>`, gera o `.deb` e publica o release do GitHub com o
pacote anexado — nenhum passo manual. Se o workflow não disparar (ex.:
merge feito por bot), acione-o à mão com `gh workflow run Release`; a
checagem de versão inédita evita release duplicado.

### Teste E2E headless

`scripts/e2e.sh` exercita o pipeline real do Ditado de ponta a ponta —
Daemon real, whisper.cpp, microfone virtual PipeWire, fala sintetizada —
num ambiente isolado, sem tocar na sua sessão:

```bash
./scripts/e2e.sh
```

Dependências: `pipewire wireplumber pipewire-pulse pulseaudio-utils
wl-clipboard libespeak-ng1 python3 curl`. O teste tem três estágios:
os sinais D-Bus de estado (obrigatório), a Entrega no clipboard (pulado
com aviso se `sway` não estiver instalado) e o colar simulado (pulado
se não houver acesso a `/dev/uinput`). No CI ele roda como o job `e2e`,
separado e **não-bloqueante** enquanto estabiliza (ver issue #24). Mais
detalhes em `CONTEXT.md`.

Detalhes de arquitetura, contratos D-Bus (extensão GNOME, Overlay) e
permissões de `uinput` estão em [`CONTEXT.md`](CONTEXT.md). Decisões de
design registradas como ADRs ficam em [`docs/adr/`](docs/adr/).
