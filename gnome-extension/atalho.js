// Lógica pura de atalho da tela de Preferências: normalização de aceleradores
// e detecção de conflito. Sem imports de `gi://` de propósito — assim roda no
// Node (`node --test gnome-extension/tests/`) e serve de base testável para o
// widget de captura em `preferencias_ui.js`, que faz a parte Gio/GTK.
//
// Um "acelerador" é a string no formato do gsettings/GTK, tipo
// `<Control><Alt>space` ou `<Primary><Alt>d`. O EverVox delega a tecla ao
// GNOME (um custom-keybinding que roda `evervox toggle`); se a combinação já
// pertence a um atalho nativo, o Mutter recusa o grab e a tecla nunca dispara.
// Comparar aceleradores exige normalizar sinônimos e ordem dos modificadores.

// Sinônimos de modificadores que o GNOME aceita como equivalentes. `Primary` e
// `Ctrl` são as duas grafias alternativas de `Control` que aparecem nos
// schemas nativos (ex.: show-desktop usa `<Primary><Alt>d`), enquanto o GTK
// emite `<Control>` — sem unificar, o mesmo atalho passaria despercebido.
const SINONIMOS_MODIFICADOR = {
    primary: 'control',
    ctrl: 'control',
    control: 'control',
    super: 'super',
    alt: 'alt',
    shift: 'shift',
    meta: 'meta',
    hyper: 'hyper',
};

/**
 * Normaliza um acelerador para uma forma canônica comparável: modificadores em
 * minúsculas, sinônimos unificados (Primary/Ctrl → control) e ordenados; a
 * tecla final (keysym) preservada como está (é sensível a maiúsculas —
 * `Left` a seta, `d` a letra). Devolve '' para acelerador vazio/em branco.
 */
export function normalizarAcelerador(acelerador) {
    if (!acelerador || acelerador.trim() === '')
        return '';

    const modificadores = [];
    let resto = acelerador;
    const regexModificador = /^<([^>]+)>/;
    let casou;
    while ((casou = resto.match(regexModificador)) !== null) {
        const bruto = casou[1].toLowerCase();
        modificadores.push(SINONIMOS_MODIFICADOR[bruto] ?? bruto);
        resto = resto.slice(casou[0].length);
    }

    const tecla = resto.trim();
    // dedup + ordena para tornar a ordem dos modificadores irrelevante
    const modsUnicos = [...new Set(modificadores)].sort();
    return `${modsUnicos.join('+')}|${tecla}`;
}

/**
 * Procura, na lista de atalhos do sistema, um cujo acelerador coincida (após
 * normalização) com `acelerador`. Devolve a entrada em conflito
 * (`{caminho, descricao, acelerador}`) ou `null` se estiver livre.
 *
 * `caminhoIgnorado` (opcional) pula a própria entrada do EverVox, para que o
 * atalho já registrado não se acuse como conflito de si mesmo.
 */
export function detectarConflito(acelerador, atalhos, caminhoIgnorado = null) {
    const alvo = normalizarAcelerador(acelerador);
    if (alvo === '')
        return null;

    for (const atalho of atalhos) {
        if (caminhoIgnorado !== null && atalho.caminho === caminhoIgnorado)
            continue;
        if (normalizarAcelerador(atalho.acelerador) === alvo)
            return atalho;
    }
    return null;
}
