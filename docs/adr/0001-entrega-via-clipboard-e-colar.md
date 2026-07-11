# Entrega via clipboard + colar simulado, não digitação sintética

No GNOME/Wayland não existe injeção livre de eventos de teclado (sem equivalente ao XTest do X11), e digitação tecla a tecla via uinput (ydotool) é lenta em textos longos e frágil com acentos do pt-br. Decidimos entregar o texto copiando para o clipboard, simulando o atalho de colar (Ctrl+V, ou Ctrl+Shift+V quando o app focado é um terminal) e restaurando o clipboard anterior.

## Consequences

- O daemon precisa saber qual app está focado para escolher o atalho de colar. O GNOME não expõe isso por API pública, então o EverVox instala uma extensão própria mínima do GNOME Shell que responde o app focado via D-Bus — um componente extra a manter a cada versão do GNOME (a mesma extensão hospeda o Overlay de estado).
- A simulação do atalho de colar em si ainda usa uinput, mas é uma única combinação de teclas — sem problema de acentos.
- Apps que bloqueiam colar (raros) não funcionarão; o texto permanece no clipboard como fallback manual.
