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
Pós-processamento opcional da Transcrição crua por LLM: remove hesitações, corrige gramática e pontuação. Fica no caminho crítico do Ditado, limitada por timeout.
_Avoid_: interpretação, correção (interpretação sugere comandos de voz, que estão fora do escopo)

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
