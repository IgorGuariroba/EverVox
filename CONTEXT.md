# EverVox

Serviço de ditado por voz para Ubuntu (GNOME/Wayland): um atalho de teclado captura a fala em qualquer app, transcreve, limpa o texto via LLM e o entrega ao campo de texto focado. Inspirado no FluidVoice (macOS).

## Language

**Ditado**:
Uma unidade completa de uso: do toggle que inicia a gravação até o texto entregue no app focado.
_Avoid_: sessão, gravação (gravação é só a captura de áudio, uma etapa do Ditado)

**Toggle**:
O único atalho de teclado do EverVox. O primeiro acionamento inicia a gravação; o segundo encerra e dispara o restante do pipeline.
_Avoid_: push-to-talk, hotkey de gravar/parar (não existem dois atalhos)

**Transcrição crua**:
O texto literal produzido pelo engine de STT, antes de qualquer limpeza. É o fallback entregue quando a Limpeza falha ou estoura o timeout.
_Avoid_: rascunho, texto bruto

**Limpeza**:
Pós-processamento opcional da Transcrição crua por LLM: remove hesitações, corrige gramática e pontuação, orientado pelas Instruções da Limpeza e pelo Vocabulário. Fica no caminho crítico do Ditado, limitada por timeout.
_Avoid_: interpretação, correção (interpretação sugere comandos de voz, que estão fora do escopo)

**Tradução**:
Etapa opcional do Ditado que converte o texto do Idioma de entrada para o Idioma de saída. Independente da Limpeza (cada uma liga/desliga sozinha); em falha ou timeout, a Transcrição crua é entregue mesmo assim.
_Avoid_: translate (o modo do Whisper, que só verte para inglês)

**Idioma de entrada**:
O idioma falado na Gravação, que orienta o Engine na transcrição.
_Avoid_: idioma (sozinho — ambíguo desde que entrada e saída podem diferir)

**Idioma de saída**:
O idioma do texto entregue ao app focado. Quando igual ao Idioma de entrada, não há Tradução.

**Vocabulário**:
Termos do usuário (nomes próprios, jargão) que orientam tanto o Engine quanto a Limpeza para grafias corretas.
_Avoid_: dicionário, glossário (glossário é este documento)

**Instruções da Limpeza**:
Texto livre do usuário que estende o comportamento da Limpeza (ex.: "expanda siglas", "mantenha tom informal").
_Avoid_: prompt (detalhe de implementação)

**Pontuação falada**:
Marcas ditadas em voz ("vírgula", "nova linha") que a Limpeza converte nos caracteres correspondentes. É formatação de ditado, não comando de voz — interpretação de intenção segue fora do escopo.
_Avoid_: comandos de voz, comandos ditados

**Preferências**:
A interface gráfica de configuração do EverVox, hospedada nas prefs da extensão GNOME.
_Avoid_: settings, painel de controle

**Engine**:
O motor de STT que transcreve o áudio, escolhido por configuração estática: `local` (Whisper na máquina) ou `cloud` (API externa). Nunca alterna sozinho em tempo de execução.
_Avoid_: backend, provider

**Entrega**:
A etapa final do Ditado: o texto entra no app focado via clipboard + colar simulado, com restauração do clipboard anterior.
_Avoid_: digitação, injeção de teclas (não digitamos tecla a tecla)

**Daemon**:
O processo residente do EverVox que mantém o modelo carregado e executa o pipeline. O Toggle apenas o sinaliza.

**Overlay**:
O indicador visual flutuante (na extensão GNOME) que mostra o estado do Ditado: gravando ou processando.
_Avoid_: tray, ícone de status

## Dev setup

Após clonar, ative os git hooks versionados (fmt/clippy no commit, testes no push):

```bash
git config core.hooksPath .githooks
```

Push direto para `main` é bloqueado (hook local + ruleset no GitHub); todo merge entra por PR com o check `ci` verde.

## Permissões de uinput (colar simulado)

A Entrega simula `Ctrl+V` criando um teclado virtual via `uinput` (ver ADR 0001), o que exige acesso de escrita a `/dev/uinput`. Sem isso o Daemon segue funcionando — o texto do Ditado só fica no clipboard, sem colar automático — mas para o colar simulado funcionar:

```bash
# Regra udev: /dev/uinput acessível pelo grupo "input"
echo 'KERNEL=="uinput", GROUP="input", MODE="0660"' | sudo tee /etc/udev/rules.d/99-evervox-uinput.rules
sudo udevadm control --reload-rules && sudo udevadm trigger

# Usuário no grupo "input" (relogar a sessão depois)
sudo usermod -aG input "$USER"
```

O Daemon verifica o acesso a `/dev/uinput` na inicialização e notifica claramente se as permissões estiverem ausentes.

## Extensão GNOME (app focado)

A Entrega decide entre `Ctrl+V` e `Ctrl+Shift+V` (terminais) consultando o app
focado via D-Bus, exposto pela extensão mínima em `gnome-extension/` (ver ADR
0001). Sem a extensão instalada e habilitada, o Daemon degrada para `Ctrl+V`
sem erro fatal. O instalador automático fica para outro ticket; para
desenvolver/testar manualmente:

```bash
ln -s "$(pwd)/gnome-extension" ~/.local/share/gnome-shell/extensions/evervox@evervox.local
# Wayland: relogar a sessão. X11: Alt+F2, r, Enter.
gnome-extensions enable evervox@evervox.local
```

Contrato D-Bus (mantenha `gnome-extension/extension.js` e
`crates/daemon/src/foco.rs` em sincronia se mudar):

- destino: `org.gnome.Shell` (a extensão não tem nome de barramento próprio)
- objeto: `/com/evervox/Extensao`
- interface: `com.evervox.Extensao1`
- método: `AppFocado() -> s` (WM_CLASS da janela focada, ou vazio)

A lista de identificadores tratados como terminal é configurável em
`terminais_conhecidos` no `config.toml` do Daemon.

## Engine cloud (OpenAI) e chaves de API

Com `engine = "cloud"` no `config.toml`, o Ditado é transcrito pela API de
transcrição de áudio da OpenAI em vez do whisper.cpp local; `engine =
"local"` (o default) continua usando o Engine local. A escolha é lida uma
única vez na inicialização do Daemon — trocar de Engine exige reiniciá-lo.

A chave de API nunca fica em config, log ou variável de ambiente: ela é
guardada no GNOME Keyring (Secret Service) via

```bash
evervox set-key openai
```

que lê a chave de forma oculta no terminal e a salva; o Daemon a lê de lá ao
preparar o Engine cloud. Sem chave salva, o Daemon falha na inicialização com
uma mensagem instruindo a rodar `set-key`. Falha de rede ou da API cloud não
cai silenciosamente para o Engine local — vira uma notificação de falha do
Ditado, igual a qualquer outra falha do Engine.
