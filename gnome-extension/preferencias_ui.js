// UI das Preferências do EverVox (issue #47): módulo compartilhado entre o
// `prefs.js` da extensão (aberto pelo app Extensões do GNOME) e o
// `aplicativo_preferencias.js` standalone (aberto pelo lançador, sem
// depender de o Shell enxergar a extensão). Edita o `config.toml` do Daemon
// diretamente e gerencia chaves de API no GNOME Keyring via Secret Service
// (libsecret). Ao salvar, chama o método de recarga do Daemon — campos
// quentes (idiomas, Limpeza inteira, Vocabulário, terminais conhecidos)
// aplicam na hora; trocar Engine ou modelo local exige reiniciar o Daemon,
// e a UI avisa e oferece o restart.
//
// Sem o Daemon rodando, a CLI + `config.toml` seguem como fallback completo
// (ver `CONTEXT.md`): esta tela só facilita a edição, nunca é o único
// caminho.
//
// Contrato D-Bus da recarga (mantenha em sincronia com
// `crates/daemon/src/main.rs::DaemonService::recarregar_config` e
// `CONTEXT.md`):
//   destino:   com.evervox.Daemon
//   objeto:    /com/evervox/Daemon
//   interface: com.evervox.Daemon1
//   método:    RecarregarConfig() -> s   ("ok" | "restart_necessario" | "erro: ...")
//
// Atributos do Keyring (mantenha em sincronia com `crates/segredo/src/lib.rs`):
// o backend Linux do crate `keyring` (`zbus-secret-service-keyring-store`)
// grava cada chave no Secret Service com exatamente os atributos
// `{service: "evervox", username: <provedor>}`, sem atributo `target` nem
// `xdg:schema`. Por isso o `Secret.Schema` abaixo usa
// `Secret.SchemaFlags.DONT_MATCH_NAME` — sem essa flag, libsecret exigiria um
// atributo `xdg:schema` que os itens gravados pelo Rust não têm, e a UI
// nunca encontraria as chaves salvas via `evervox set-key`.
//
// As versões dos GI ficam explícitas nos imports para o módulo funcionar
// igual nos dois hospedeiros: o processo de prefs do Shell já as teria
// fixado, mas o gjs standalone não.

import Adw from 'gi://Adw?version=1';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Gtk from 'gi://Gtk?version=4.0';
import Secret from 'gi://Secret?version=1';

const DAEMON_SERVICE_NAME = 'com.evervox.Daemon';
const DAEMON_OBJECT_PATH = '/com/evervox/Daemon';
const DAEMON_INTERFACE_NAME = 'com.evervox.Daemon1';

const SERVICO_KEYRING = 'evervox';
const PROVEDORES_DE_CHAVE = ['openai', 'anthropic'];
const SEGREDO_SCHEMA = new Secret.Schema(
    'com.evervox.Segredo',
    Secret.SchemaFlags.DONT_MATCH_NAME,
    {
        service: Secret.SchemaAttributeType.STRING,
        username: Secret.SchemaAttributeType.STRING,
    }
);

/** Caminho do `config.toml`, espelhando `crate::xdg::resolver` do Daemon. */
function caminhoConfig() {
    const base = GLib.getenv('XDG_CONFIG_HOME') ?? GLib.build_filenamev([GLib.get_home_dir(), '.config']);
    return GLib.build_filenamev([base, 'evervox', 'config.toml']);
}

/**
 * Parser mínimo de TOML suficiente para o schema plano do `config.toml` do
 * EverVox (strings, booleanos, números, arrays de string e uma única seção
 * `[limpeza]`) — não é um parser de TOML genérico. Opera sobre o texto
 * inteiro (não linha a linha) para lidar com arrays que o
 * `toml::to_string_pretty` do Rust grava em várias linhas.
 */
function analisarToml(texto) {
    const raiz = {};
    let secaoAtual = raiz;
    let i = 0;
    const n = texto.length;

    function pularEspacosEComentarios() {
        for (;;) {
            while (i < n && /\s/.test(texto[i]))
                i++;
            if (texto[i] === '#') {
                while (i < n && texto[i] !== '\n')
                    i++;
            } else {
                break;
            }
        }
    }

    function lerString() {
        i++; // abre aspas
        let resultado = '';
        while (i < n && texto[i] !== '"') {
            if (texto[i] === '\\') {
                i++;
                const escapado = texto[i];
                resultado += escapado === 'n' ? '\n' : escapado === 't' ? '\t' : escapado;
            } else {
                resultado += texto[i];
            }
            i++;
        }
        i++; // fecha aspas
        return resultado;
    }

    function lerValor() {
        pularEspacosEComentarios();
        if (texto[i] === '"')
            return lerString();
        if (texto.startsWith('true', i)) {
            i += 4;
            return true;
        }
        if (texto.startsWith('false', i)) {
            i += 5;
            return false;
        }
        if (texto[i] === '[') {
            i++;
            const itens = [];
            for (;;) {
                pularEspacosEComentarios();
                if (texto[i] === ']') {
                    i++;
                    break;
                }
                itens.push(lerValor());
                pularEspacosEComentarios();
                if (texto[i] === ',')
                    i++;
            }
            return itens;
        }
        const inicio = i;
        while (i < n && /[0-9.\-]/.test(texto[i]))
            i++;
        return Number(texto.slice(inicio, i));
    }

    while (i < n) {
        pularEspacosEComentarios();
        if (i >= n)
            break;
        if (texto[i] === '[') {
            i++;
            const inicio = i;
            while (i < n && texto[i] !== ']')
                i++;
            const nomeSecao = texto.slice(inicio, i);
            i++; // ]
            raiz[nomeSecao] = {};
            secaoAtual = raiz[nomeSecao];
            continue;
        }
        const inicio = i;
        while (i < n && /[A-Za-z0-9_]/.test(texto[i]))
            i++;
        const chave = texto.slice(inicio, i);
        if (!chave)
            break;
        pularEspacosEComentarios();
        if (texto[i] !== '=')
            break;
        i++; // =
        secaoAtual[chave] = lerValor();
    }

    return raiz;
}

function escaparString(valor) {
    return valor.replace(/\\/g, '\\\\').replace(/"/g, '\\"').replace(/\n/g, '\\n');
}

function serializarValor(valor) {
    if (typeof valor === 'string')
        return `"${escaparString(valor)}"`;
    if (typeof valor === 'boolean' || typeof valor === 'number')
        return String(valor);
    if (Array.isArray(valor))
        return `[${valor.map(serializarValor).join(', ')}]`;
    throw new Error(`valor TOML não suportado: ${valor}`);
}

/** Serializa a config de volta para TOML, no formato que `toml::from_str` do
 * Rust deserializa de volta (não precisa ser byte-idêntico ao que
 * `toml::to_string_pretty` produziria — só válido). */
function serializarConfig(config) {
    const linhas = [];
    for (const chave of ['idioma_entrada', 'idioma_saida', 'modelo_local', 'engine', 'terminais_conhecidos', 'vocabulario'])
        linhas.push(`${chave} = ${serializarValor(config[chave])}`);
    linhas.push('', '[limpeza]');
    const limpeza = config.limpeza;
    for (const chave of ['habilitada', 'provedor', 'modelo', 'timeout_ms', 'instrucoes', 'pontuacao_falada'])
        linhas.push(`${chave} = ${serializarValor(limpeza[chave])}`);
    return `${linhas.join('\n')}\n`;
}

/** Lê o `config.toml`; se não existir ainda, devolve os defaults do Daemon
 * (`Config::default()` em `crates/daemon/src/config.rs`) — a Preferências
 * nunca cria o arquivo sozinha, só o Daemon faz isso na primeira execução. */
function lerConfig() {
    const arquivo = Gio.File.new_for_path(caminhoConfig());
    const padrao = {
        idioma_entrada: 'pt',
        idioma_saida: 'pt',
        modelo_local: 'base',
        engine: 'local',
        terminais_conhecidos: [
            'gnome-terminal-server', 'org.gnome.terminal', 'org.gnome.console',
            'kgx', 'alacritty', 'kitty', 'konsole', 'xterm', 'tilix', 'wezterm',
        ],
        vocabulario: [],
        limpeza: {
            habilitada: false,
            provedor: 'openai',
            modelo: 'gpt-4o-mini',
            timeout_ms: 4000,
            instrucoes: '',
            pontuacao_falada: true,
        },
    };
    if (!arquivo.query_exists(null))
        return padrao;
    const [ok, conteudo] = arquivo.load_contents(null);
    if (!ok)
        return padrao;
    const texto = new TextDecoder('utf-8').decode(conteudo);
    const lido = analisarToml(texto);
    return {...padrao, ...lido, limpeza: {...padrao.limpeza, ...lido.limpeza}};
}

function salvarConfig(config) {
    const arquivo = Gio.File.new_for_path(caminhoConfig());
    const diretorio = arquivo.get_parent();
    // `make_directory_with_parents` lança `Gio.IOErrorEnum.EXISTS` se o
    // diretório já existir (o caso comum: o Daemon já rodou antes) — só
    // criamos se realmente faltar.
    if (diretorio && !diretorio.query_exists(null))
        diretorio.make_directory_with_parents(null);
    const bytes = new TextEncoder().encode(serializarConfig(config));
    arquivo.replace_contents(bytes, null, false, Gio.FileCreateFlags.REPLACE_DESTINATION, null);
}

/** Chama `RecarregarConfig()` no Daemon. Devolve `null` se o Daemon não
 * estiver acessível pelo D-Bus de sessão (fallback: as mudanças já foram
 * salvas no `config.toml` e valem no próximo start do Daemon). */
function recarregarConfigNoDaemon() {
    try {
        const proxy = Gio.DBusProxy.new_for_bus_sync(
            Gio.BusType.SESSION,
            Gio.DBusProxyFlags.DO_NOT_LOAD_PROPERTIES | Gio.DBusProxyFlags.DO_NOT_CONNECT_SIGNALS,
            null,
            DAEMON_SERVICE_NAME,
            DAEMON_OBJECT_PATH,
            DAEMON_INTERFACE_NAME,
            null
        );
        const resultado = proxy.call_sync('RecarregarConfig', null, Gio.DBusCallFlags.NONE, -1, null);
        return resultado.deep_unpack()[0];
    } catch (erro) {
        logError(erro, 'evervox: falha ao chamar RecarregarConfig no Daemon');
        return null;
    }
}

function reiniciarDaemon() {
    try {
        Gio.Subprocess.new(['systemctl', '--user', 'restart', 'evervox'], Gio.SubprocessFlags.NONE);
    } catch (erro) {
        logError(erro, 'evervox: falha ao reiniciar o Daemon via systemctl --user');
    }
}

function grupoIdiomas(config) {
    const grupo = new Adw.PreferencesGroup({title: 'Idiomas'});

    const entrada = new Adw.EntryRow({title: 'Idioma de entrada'});
    entrada.set_text(config.idioma_entrada);
    entrada.connect('notify::text', () => config.idioma_entrada = entrada.get_text());
    grupo.add(entrada);

    const saida = new Adw.EntryRow({title: 'Idioma de saída'});
    saida.set_text(config.idioma_saida);
    saida.connect('notify::text', () => config.idioma_saida = saida.get_text());
    grupo.add(saida);

    return grupo;
}

function grupoChaves() {
    const grupo = new Adw.PreferencesGroup({
        title: 'Chaves de API',
        description: 'Chaves nunca são exibidas depois de salvas: só o GNOME Keyring as guarda.',
    });

    for (const provedor of PROVEDORES_DE_CHAVE)
        grupo.add(linhaDeChave(provedor));

    return grupo;
}

function linhaDeChave(provedor) {
    const linha = new Adw.ActionRow({title: provedor});

    const atualizarSubtitulo = () => {
        const chave = Secret.password_lookup_sync(SEGREDO_SCHEMA, {service: SERVICO_KEYRING, username: provedor}, null);
        linha.set_subtitle(chave ? 'Chave salva ✓' : 'Nenhuma chave salva');
    };
    atualizarSubtitulo();

    const botaoSubstituir = new Gtk.Button({label: 'Substituir', valign: Gtk.Align.CENTER});
    botaoSubstituir.connect('clicked', () => dialogoDeChave(linha, provedor, atualizarSubtitulo));
    linha.add_suffix(botaoSubstituir);

    const botaoRemover = new Gtk.Button({label: 'Remover', valign: Gtk.Align.CENTER});
    botaoRemover.connect('clicked', () => {
        Secret.password_clear_sync(SEGREDO_SCHEMA, {service: SERVICO_KEYRING, username: provedor}, null);
        atualizarSubtitulo();
    });
    linha.add_suffix(botaoRemover);

    return linha;
}

function dialogoDeChave(linhaPai, provedor, aoSalvar) {
    const entradaSenha = new Adw.PasswordEntryRow({title: 'Chave de API'});
    const grupo = new Adw.PreferencesGroup();
    grupo.add(entradaSenha);

    const dialogo = new Adw.MessageDialog({
        transient_for: linhaPai.get_root(),
        heading: `Chave de API — ${provedor}`,
        extra_child: grupo,
    });
    dialogo.add_response('cancelar', 'Cancelar');
    dialogo.add_response('salvar', 'Salvar');
    dialogo.set_response_appearance('salvar', Adw.ResponseAppearance.SUGGESTED);
    dialogo.connect('response', (_dialogo, resposta) => {
        if (resposta === 'salvar' && entradaSenha.get_text().trim() !== '') {
            Secret.password_store_sync(
                SEGREDO_SCHEMA,
                {service: SERVICO_KEYRING, username: provedor},
                Secret.COLLECTION_DEFAULT,
                `EverVox: chave de API (${provedor})`,
                entradaSenha.get_text(),
                null
            );
            aoSalvar();
        }
    });
    dialogo.present();
}

function grupoEngine(config) {
    const grupo = new Adw.PreferencesGroup({
        title: 'Engine',
        description: 'Trocar o Engine ou o modelo local exige reiniciar o Daemon.',
    });

    const engine = new Adw.ComboRow({
        title: 'Engine de transcrição',
        model: Gtk.StringList.new(['local', 'cloud']),
        selected: config.engine === 'cloud' ? 1 : 0,
    });
    engine.connect('notify::selected', () => config.engine = engine.selected === 1 ? 'cloud' : 'local');
    grupo.add(engine);

    const modelo = new Adw.EntryRow({title: 'Modelo local (whisper.cpp)'});
    modelo.set_text(config.modelo_local);
    modelo.connect('notify::text', () => config.modelo_local = modelo.get_text());
    grupo.add(modelo);

    return grupo;
}

function grupoLimpeza(config) {
    const limpeza = config.limpeza;
    const grupo = new Adw.PreferencesGroup({title: 'Limpeza'});

    const habilitada = new Adw.SwitchRow({title: 'Limpeza ligada', active: limpeza.habilitada});
    habilitada.connect('notify::active', () => limpeza.habilitada = habilitada.active);
    grupo.add(habilitada);

    const provedor = new Adw.ComboRow({
        title: 'Provedor',
        model: Gtk.StringList.new(['openai', 'anthropic']),
        selected: limpeza.provedor === 'anthropic' ? 1 : 0,
    });
    provedor.connect('notify::selected', () => limpeza.provedor = provedor.selected === 1 ? 'anthropic' : 'openai');
    grupo.add(provedor);

    const modelo = new Adw.EntryRow({title: 'Modelo'});
    modelo.set_text(limpeza.modelo);
    modelo.connect('notify::text', () => limpeza.modelo = modelo.get_text());
    grupo.add(modelo);

    const timeout = new Adw.SpinRow({
        title: 'Timeout (ms)',
        adjustment: new Gtk.Adjustment({lower: 500, upper: 60000, step_increment: 500}),
        value: limpeza.timeout_ms,
    });
    timeout.connect('notify::value', () => limpeza.timeout_ms = Math.round(timeout.value));
    grupo.add(timeout);

    const instrucoes = new Adw.EntryRow({title: 'Instruções da Limpeza'});
    instrucoes.set_text(limpeza.instrucoes);
    instrucoes.connect('notify::text', () => limpeza.instrucoes = instrucoes.get_text());
    grupo.add(instrucoes);

    const vocabulario = new Adw.EntryRow({title: 'Vocabulário (separado por vírgula)'});
    vocabulario.set_text(config.vocabulario.join(', '));
    vocabulario.connect('notify::text', () => {
        config.vocabulario = vocabulario.get_text()
            .split(',')
            .map(termo => termo.trim())
            .filter(termo => termo !== '');
    });
    grupo.add(vocabulario);

    const pontuacaoFalada = new Adw.SwitchRow({title: 'Pontuação falada', active: limpeza.pontuacao_falada});
    pontuacaoFalada.connect('notify::active', () => limpeza.pontuacao_falada = pontuacaoFalada.active);
    grupo.add(pontuacaoFalada);

    return grupo;
}

function salvar(window, config) {
    salvarConfig(config);
    const resultado = recarregarConfigNoDaemon();

    if (resultado === null) {
        avisar(window, 'Daemon não está rodando',
            'As mudanças foram salvas no config.toml e serão aplicadas no próximo início do Daemon.');
        return;
    }
    if (resultado.startsWith('erro:')) {
        avisar(window, 'Falha ao recarregar', resultado);
        return;
    }
    if (resultado === 'restart_necessario') {
        confirmarRestart(window);
        return;
    }
    avisar(window, 'Preferências salvas', 'Aplicadas sem precisar reiniciar o Daemon.');
}

function confirmarRestart(window) {
    const dialogo = new Adw.MessageDialog({
        transient_for: window,
        heading: 'Reiniciar o Daemon?',
        body: 'Trocar o Engine ou o modelo local só vale depois de reiniciar o Daemon.',
    });
    dialogo.add_response('depois', 'Reiniciar depois');
    dialogo.add_response('agora', 'Reiniciar agora');
    dialogo.set_response_appearance('agora', Adw.ResponseAppearance.SUGGESTED);
    dialogo.connect('response', (_dialogo, resposta) => {
        if (resposta === 'agora')
            reiniciarDaemon();
    });
    dialogo.present();
}

function avisar(window, titulo, corpo) {
    const dialogo = new Adw.MessageDialog({transient_for: window, heading: titulo, body: corpo});
    dialogo.add_response('ok', 'OK');
    dialogo.present();
}

/**
 * Monta a página de Preferências dentro de `window` (qualquer
 * Adw.PreferencesWindow: a das prefs da extensão ou a do app standalone).
 * `gruposIniciais` entram antes de tudo na mesma página — é como o app
 * standalone injeta o aviso de "extensão pendente de login".
 */
export function preencherJanelaDePreferencias(window, gruposIniciais = []) {
    const config = lerConfig();

    const pagina = new Adw.PreferencesPage();
    for (const grupo of gruposIniciais)
        pagina.add(grupo);
    pagina.add(grupoIdiomas(config));
    pagina.add(grupoChaves());
    pagina.add(grupoEngine(config));
    pagina.add(grupoLimpeza(config));
    window.add(pagina);

    const grupoSalvar = new Adw.PreferencesGroup();
    const linhaSalvar = new Adw.ActionRow({title: 'Salvar Preferências'});
    const botaoSalvar = new Gtk.Button({
        label: 'Salvar',
        valign: Gtk.Align.CENTER,
        css_classes: ['suggested-action'],
    });
    botaoSalvar.connect('clicked', () => salvar(window, config));
    linhaSalvar.add_suffix(botaoSalvar);
    grupoSalvar.add(linhaSalvar);
    pagina.add(grupoSalvar);
}
