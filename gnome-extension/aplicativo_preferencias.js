// Preferências do EverVox como app GJS standalone (issue #47): mesma UI das
// prefs da extensão (`preferencias_ui.js`), mas aberta direto do disco via
// `gjs -m`, sem pedir nada ao GNOME Shell — funciona logo depois de
// instalar ou atualizar o pacote, quando o Shell (Wayland) ainda não
// enxerga a extensão e o `gnome-extensions prefs` falharia. É o que o
// lançador `evervox-preferencias` executa.
//
// Quando o Shell ainda não carregou a extensão (Overlay e detecção de
// terminal pendentes até o próximo login), um aviso não-bloqueante no topo
// oferece encerrar a sessão — o resto das Preferências segue utilizável.

import Adw from 'gi://Adw?version=1';
import Gio from 'gi://Gio';
import Gtk from 'gi://Gtk?version=4.0';

import {preencherJanelaDePreferencias} from './preferencias_ui.js';

const EXT_UUID = 'evervox@evervox.local';

/** O Shell da sessão já enxerga a extensão? (`gnome-extensions info` sai 0.) */
function shellEnxergaExtensao() {
    try {
        const processo = Gio.Subprocess.new(
            ['gnome-extensions', 'info', EXT_UUID],
            Gio.SubprocessFlags.STDOUT_SILENCE | Gio.SubprocessFlags.STDERR_SILENCE
        );
        processo.wait(null);
        return processo.get_successful();
    } catch (_erro) {
        // Sem `gnome-extensions` no PATH não dá para saber; não avisa.
        return true;
    }
}

/** Aviso de extensão pendente de login, com botão de encerrar a sessão. */
function grupoAvisoDeLogin() {
    const grupo = new Adw.PreferencesGroup();
    const linha = new Adw.ActionRow({
        title: 'Extensão do EverVox ainda não carregada',
        subtitle: 'O GNOME só carrega a extensão (Overlay de gravação) no próximo login. As Preferências e o ditado já funcionam.',
    });
    const botao = new Gtk.Button({label: 'Encerrar sessão…', valign: Gtk.Align.CENTER});
    botao.connect('clicked', () => {
        // O gnome-session-quit mostra a confirmação do próprio GNOME.
        Gio.Subprocess.new(['gnome-session-quit', '--logout'], Gio.SubprocessFlags.NONE);
    });
    linha.add_suffix(botao);
    grupo.add(linha);
    return grupo;
}

const app = new Adw.Application({application_id: 'com.evervox.Preferencias'});

app.connect('activate', () => {
    const janela = new Adw.PreferencesWindow({
        application: app,
        title: 'EverVox — Preferências',
    });
    const gruposIniciais = shellEnxergaExtensao() ? [] : [grupoAvisoDeLogin()];
    preencherJanelaDePreferencias(janela, gruposIniciais);
    janela.present();
});

app.run([]);
