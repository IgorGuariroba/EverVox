// Preferências do EverVox (issue #19, ADR 0004) abertas pelo app Extensões
// do GNOME. Toda a UI vive em `preferencias_ui.js` (issue #47), módulo
// compartilhado com o `aplicativo_preferencias.js` standalone — este arquivo
// é só o adaptador para o contrato de prefs do Shell.

import {ExtensionPreferences} from 'resource:///org/gnome/Shell/Extensions/js/extensions/prefs.js';

import {preencherJanelaDePreferencias} from './preferencias_ui.js';

export default class EverVoxPreferences extends ExtensionPreferences {
    fillPreferencesWindow(window) {
        preencherJanelaDePreferencias(window);
    }
}
