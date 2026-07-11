// Extensão GNOME Shell mínima do EverVox (ver ADR 0001 e issue #6): expõe o
// identificador (WM_CLASS) do app focado via D-Bus, para o Daemon decidir
// entre Ctrl+V e Ctrl+Shift+V na Entrega. Não possui um nome de barramento
// próprio — o objeto é exportado na conexão de sessão que o próprio GNOME
// Shell já é dono de `org.gnome.Shell`, então é isso que o Daemon disca.
//
// Endereço D-Bus (deve ficar em sincronia com `crates/daemon/src/foco.rs`):
//   destino:   org.gnome.Shell
//   objeto:    /com/evervox/Extensao
//   interface: com.evervox.Extensao1
//   método:    AppFocado() -> s

import Gio from 'gi://Gio';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

const OBJECT_PATH = '/com/evervox/Extensao';

const IFACE_XML = `
<node>
  <interface name="com.evervox.Extensao1">
    <method name="AppFocado">
      <arg type="s" direction="out" name="app_id"/>
    </method>
  </interface>
</node>`;

export default class EverVoxExtension extends Extension {
    enable() {
        this._dbusImpl = Gio.DBusExportedObject.wrapJSObject(IFACE_XML, this);
        this._dbusImpl.export(Gio.DBus.session, OBJECT_PATH);
    }

    disable() {
        this._dbusImpl?.unexport();
        this._dbusImpl = null;
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
}
