// Testes da lógica pura de atalho (sem GTK/Gio), roda com `node --test`.
// Cobre a normalização de aceleradores e a detecção de conflito usadas pela
// tela de Preferências para avisar quando a combinação escolhida já pertence
// a um atalho nativo do GNOME.

import {test} from 'node:test';
import assert from 'node:assert/strict';

import {normalizarAcelerador, detectarConflito} from '../atalho.js';

test('normaliza <Primary> e <Ctrl> para o mesmo que <Control>', () => {
    assert.equal(
        normalizarAcelerador('<Primary><Alt>d'),
        normalizarAcelerador('<Control><Alt>d')
    );
    assert.equal(
        normalizarAcelerador('<Ctrl><Super>0'),
        normalizarAcelerador('<Control><Super>0')
    );
});

test('normalização independe da ordem dos modificadores', () => {
    assert.equal(
        normalizarAcelerador('<Super><Alt>Left'),
        normalizarAcelerador('<Alt><Super>Left')
    );
});

test('a tecla (keysym) é sensível a maiúsculas e diferencia atalhos', () => {
    assert.notEqual(
        normalizarAcelerador('<Control><Alt>d'),
        normalizarAcelerador('<Control><Alt>j')
    );
});

test('acelerador vazio normaliza para vazio', () => {
    assert.equal(normalizarAcelerador(''), '');
    assert.equal(normalizarAcelerador('   '), '');
});

const ATALHOS = [
    {caminho: 'wm:show-desktop', descricao: 'Mostrar a área de trabalho', acelerador: '<Primary><Alt>d'},
    {caminho: 'wm:show-desktop', descricao: 'Mostrar a área de trabalho', acelerador: '<Super>d'},
    {caminho: 'media:screensaver', descricao: 'Bloquear a tela', acelerador: '<Super>l'},
    {caminho: '.../custom-keybindings/evervox/', descricao: 'EverVox Toggle', acelerador: '<Control><Alt>space'},
];

test('detecta conflito com atalho nativo (Ctrl+Alt+D = show-desktop)', () => {
    const conflito = detectarConflito('<Control><Alt>d', ATALHOS);
    assert.ok(conflito);
    assert.equal(conflito.descricao, 'Mostrar a área de trabalho');
});

test('combinação livre não acusa conflito', () => {
    assert.equal(detectarConflito('<Control><Alt>j', ATALHOS), null);
});

test('ignora o próprio atalho do EverVox (não conta como autoconflito)', () => {
    const conflito = detectarConflito('<Control><Alt>space', ATALHOS, '.../custom-keybindings/evervox/');
    assert.equal(conflito, null);
});

test('sem o caminho ignorado, o próprio EverVox apareceria como conflito', () => {
    const conflito = detectarConflito('<Control><Alt>space', ATALHOS);
    assert.ok(conflito);
    assert.equal(conflito.descricao, 'EverVox Toggle');
});

test('acelerador vazio nunca conflita', () => {
    assert.equal(detectarConflito('', ATALHOS), null);
});
