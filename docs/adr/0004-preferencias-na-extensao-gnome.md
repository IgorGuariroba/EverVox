# Preferências nas prefs da extensão GNOME, com recarga a quente exceto Engine

A UI de configuração do EverVox (Preferências) vive nas prefs da extensão GNOME já existente (GTK4/Adwaita via `prefs.js`), em vez de um app GTK separado ou uma TUI: integra-se ao app Extensions do GNOME, não adiciona processo novo e acessa o GNOME Keyring via Secret Service para as chaves de API. Ao salvar, o Daemon é notificado por um método D-Bus de recarga e aplica na hora os campos quentes (idiomas, Instruções da Limpeza, Vocabulário, terminais conhecidos); **trocar Engine ou modelo continua exigindo reiniciar o Daemon** — preservando a decisão de Engine estático da spec — e a UI avisa e oferece o restart.

Chaves de API são **write-only** na UI: nunca voltam do Keyring para a tela. A UI mostra apenas "chave salva ✓" com ações Substituir/Remover, coerente com a regra de que nenhuma chave aparece em config, logs ou ambiente.

## Consequences

- As Preferências só existem com a extensão instalada — aceitável porque a extensão já é parte da instalação padrão (Foco e Overlay dependem dela); sem ela, CLI + `config.toml` seguem como fallback.
- O Daemon ganha um contrato D-Bus novo (recarga de config) que a extensão precisa manter em sincronia, como já ocorre com `AppFocado`.
