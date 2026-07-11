// Extensão GNOME Shell mínima do EverVox (ver ADR 0001): expõe o
// identificador (WM_CLASS) do app focado via D-Bus, para o Daemon decidir
// entre Ctrl+V e Ctrl+Shift+V na Entrega, e hospeda o Overlay de estado do
// Ditado (issue #9): um indicador flutuante que reflete os sinais D-Bus de
// estado emitidos pelo Daemon. A extensão não tem lógica de negócio própria
// — apenas expõe o app focado e desenha o que o Daemon manda.
//
// Endereço D-Bus do app focado (deve ficar em sincronia com
// `crates/daemon/src/foco.rs`):
//   destino:   org.gnome.Shell
//   objeto:    /com/evervox/Extensao
//   interface: com.evervox.Extensao1
//   método:    AppFocado() -> s
//
// Endereço D-Bus do Overlay (deve ficar em sincronia com
// `crates/core/src/lib.rs::dbus` e `crates/daemon/src/main.rs`):
//   objeto:    /com/evervox/Daemon
//   interface: com.evervox.Daemon1
//   sinal:     Estado(s) — "gravando" | "processando" | "ocioso"
//
// O sinal é emitido por uma conexão D-Bus própria do Daemon (sem nome de
// barramento reivindicado, ver `DaemonFeedback` em `crates/daemon/src/main.rs`),
// então a assinatura não filtra por remetente — só por objeto/interface/sinal.

import Gio from 'gi://Gio';
import St from 'gi://St';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';

const OBJECT_PATH = '/com/evervox/Extensao';

const IFACE_XML = `
<node>
  <interface name="com.evervox.Extensao1">
    <method name="AppFocado">
      <arg type="s" direction="out" name="app_id"/>
    </method>
  </interface>
</node>`;

const DAEMON_OBJECT_PATH = '/com/evervox/Daemon';
const DAEMON_INTERFACE_NAME = 'com.evervox.Daemon1';
const DAEMON_SIGNAL_ESTADO = 'Estado';

/** Texto do Overlay por estado; estados ausentes daqui (ex.: "ocioso") escondem o Overlay. */
const TEXTO_POR_ESTADO = {
    gravando: '\u{1F3A4} Gravando…',
    processando: '\u{23F3} Processando…',
};

export default class EverVoxExtension extends Extension {
    enable() {
        this._dbusImpl = Gio.DBusExportedObject.wrapJSObject(IFACE_XML, this);
        this._dbusImpl.export(Gio.DBus.session, OBJECT_PATH);

        this._overlay = this._criarOverlay();
        this._assinaturaEstado = Gio.DBus.session.signal_subscribe(
            null,
            DAEMON_INTERFACE_NAME,
            DAEMON_SIGNAL_ESTADO,
            DAEMON_OBJECT_PATH,
            null,
            Gio.DBusSignalFlags.NONE,
            (_conn, _sender, _path, _iface, _signal, params) => {
                const [estado] = params.deep_unpack();
                this._refletirEstado(estado);
            }
        );
    }

    disable() {
        this._dbusImpl?.unexport();
        this._dbusImpl = null;

        if (this._assinaturaEstado !== undefined) {
            Gio.DBus.session.signal_unsubscribe(this._assinaturaEstado);
            this._assinaturaEstado = undefined;
        }

        if (this._overlay) {
            Main.layoutManager.removeChrome(this._overlay);
            this._overlay.destroy();
            this._overlay = null;
        }
    }

    /**
     * Devolve o WM_CLASS da janela focada, ou string vazia se nada estiver
     * focado (ex.: overview aberto). O Daemon trata string vazia como "não é
     * terminal" — mesmo efeito de Ctrl+V padrão.
     */
    AppFocado() {
        const janela = global.display.focus_window;
        return janela ? janela.get_wm_class() ?? '' : '';
    }

    /**
     * Cria o indicador flutuante do Overlay, escondido até o primeiro sinal
     * de estado chegar. `affectsInputRegion: false` garante que ele nunca
     * rouba foco nem intercepta cliques — é puramente informativo.
     */
    _criarOverlay() {
        const overlay = new St.Label({
            style_class: 'evervox-overlay',
            reactive: false,
            can_focus: false,
            track_hover: false,
            visible: false,
        });
        Main.layoutManager.addChrome(overlay, {
            affectsInputRegion: false,
            trackFullscreen: true,
        });
        overlay.connect('notify::width', () => this._posicionar(overlay));
        return overlay;
    }

    /** Centraliza o Overlay horizontalmente, perto do topo do monitor primário. */
    _posicionar(overlay) {
        const monitor = Main.layoutManager.primaryMonitor;
        if (!monitor)
            return;
        overlay.set_position(
            monitor.x + Math.floor((monitor.width - overlay.width) / 2),
            monitor.y + 32
        );
    }

    /** Reflete o estado recebido do Daemon no Overlay: mostra ou esconde. */
    _refletirEstado(estado) {
        const texto = TEXTO_POR_ESTADO[estado];
        if (!texto) {
            this._overlay.visible = false;
            return;
        }
        this._overlay.text = texto;
        this._overlay.visible = true;
        this._posicionar(this._overlay);
    }
}
