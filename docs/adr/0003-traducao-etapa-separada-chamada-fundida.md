# Tradução como conceito separado da Limpeza, com chamada LLM fundida

O usuário pode falar num idioma e receber o texto em outro (Idioma de entrada ≠ Idioma de saída). Decidimos que Tradução e Limpeza são conceitos independentes no domínio — cada um liga/desliga sozinho, então dá para traduzir texto literal sem limpar, e vice-versa — mas, quando ambos estão ligados, o Daemon faz **uma única chamada de LLM** que limpa e traduz junto: um só timeout, latência de uma etapa. Em falha ou timeout, vale a regra de sempre: a Transcrição crua é entregue no idioma falado, com notificação discreta — o Ditado nunca fica refém da rede, mesmo que isso signifique receber o texto no idioma "errado".

## Considered Options

- **Tradução como parte da Limpeza** — rejeitada: impossibilita traduzir sem limpar (texto literal em outro idioma).
- **Duas chamadas LLM em série** (Limpar → Traduzir) — rejeitada: dobra a latência e os modos de falha no caminho crítico do Ditado (~8s no pior caso).
- **`translate` nativo do Whisper** — rejeitada como mecanismo geral: só verte para inglês; limitaria o Idioma de saída a um único valor.
