mod audio;
mod config;
mod engine_cloud;
mod engine_whisper;
mod entrega;
mod foco;
mod limpeza;
mod microfone;
mod modelo;
mod wav;
mod xdg;

use config::Engine as EngineEscolhido;
use config::ProvedorLimpeza as ProvedorLimpezaEscolhido;
use engine_cloud::EngineCloud;
use engine_whisper::EngineWhisper;
use entrega::EntregaClipboard;
use evervox_core::{
    dbus, EngineSTT, ErroMicrofone, Feedback, Limpeza, LimpezaExecucao, Machine, ResultadoToggle,
};
use foco::FocoGnome;
use microfone::MicrofoneCpal;
use notify_rust::Notification;
use std::process::Command;
use std::thread;
use tokio::sync::Mutex;
use zbus::{connection, interface};

/// Feedback sonoro, por notificação e por Overlay do Ditado completo: sons
/// do freedesktop sound theme via canberra, notificações via `notify-rust` e
/// o sinal D-Bus de estado que a extensão GNOME reflete no Overlay (ver
/// `CONTEXT.md`). Sem D-Bus de sessão disponível, os dois primeiros seguem
/// funcionando normalmente — só o Overlay não aparece.
struct DaemonFeedback {
    dbus: Option<zbus::blocking::Connection>,
}

impl DaemonFeedback {
    fn nova() -> Self {
        Self {
            dbus: zbus::blocking::Connection::session().ok(),
        }
    }

    fn play(&self, event_id: &str) {
        if let Err(erro) = Command::new("canberra-gtk-play")
            .arg("-i")
            .arg(event_id)
            .spawn()
        {
            eprintln!("evervox-daemon: falha ao tocar som '{event_id}': {erro}");
        }
    }

    fn notificar(&self, corpo: &str, urgencia: notify_rust::Urgency) {
        if let Err(erro) = Notification::new()
            .summary("EverVox")
            .body(corpo)
            .urgency(urgencia)
            .show()
        {
            eprintln!("evervox-daemon: falha ao notificar: {erro}");
        }
    }

    /// Emite o sinal de mudança de estado do Ditado que alimenta o Overlay
    /// da extensão GNOME. `iniciou_gravacao`/`encerrou_gravacao` chegam
    /// síncronas do handler D-Bus async do Toggle; os demais eventos chegam
    /// de uma thread em segundo plano (ver
    /// `evervox_core::Machine::despachar_processamento`). Para nunca
    /// bloquear a runtime Tokio com a chamada bloqueante do zbus, o envio em
    /// si roda sempre numa thread própria e de vida curta — o chamador não
    /// espera o sinal sair.
    fn emitir_estado(&self, estado: &str) {
        let Some(connection) = self.dbus.clone() else {
            return;
        };
        let estado = estado.to_string();
        thread::spawn(move || {
            if let Err(erro) = connection.emit_signal(
                None::<&str>,
                evervox_core::dbus::OBJECT_PATH,
                evervox_core::dbus::INTERFACE_NAME,
                evervox_core::dbus::SIGNAL_ESTADO,
                &estado,
            ) {
                eprintln!("evervox-daemon: falha ao emitir sinal de estado do Overlay: {erro}");
            }
        });
    }
}

/// Tamanho máximo do trecho da Transcrição exibido na notificação de
/// conclusão: notificações não são o lugar para o Ditado inteiro, e o texto
/// completo já está no clipboard.
const TAMANHO_MAXIMO_NA_NOTIFICACAO: usize = 80;

fn resumir_para_notificacao(texto: &str) -> String {
    if texto.chars().count() <= TAMANHO_MAXIMO_NA_NOTIFICACAO {
        return texto.to_string();
    }
    let resumo: String = texto.chars().take(TAMANHO_MAXIMO_NA_NOTIFICACAO).collect();
    format!("{resumo}…")
}

impl Feedback for DaemonFeedback {
    fn iniciou_gravacao(&mut self) {
        self.play("message-new-instant");
        self.emitir_estado("gravando");
    }

    fn encerrou_gravacao(&mut self) {
        self.play("complete");
        self.emitir_estado("processando");
    }

    fn concluiu_ditado(&mut self, texto: &str) {
        let resumo = resumir_para_notificacao(texto);
        self.notificar(
            &format!("Ditado concluído: \"{resumo}\""),
            notify_rust::Urgency::Normal,
        );
        self.emitir_estado("ocioso");
    }

    fn ditado_silencioso(&mut self) {
        self.notificar(
            "Nenhuma fala detectada no Ditado.",
            notify_rust::Urgency::Low,
        );
        self.emitir_estado("ocioso");
    }

    fn ditado_no_clipboard_sem_colar(&mut self, texto: &str) {
        let resumo = resumir_para_notificacao(texto);
        eprintln!("evervox-daemon: colar automático falhou, texto ficou no clipboard");
        self.notificar(
            &format!("Não foi possível colar automaticamente: \"{resumo}\" está no clipboard, cole com Ctrl+V."),
            notify_rust::Urgency::Normal,
        );
        self.emitir_estado("ocioso");
    }

    fn falha_ditado(&mut self, mensagem: &str) {
        eprintln!("evervox-daemon: {mensagem}");
        self.notificar(
            &format!("O Ditado falhou: {mensagem}"),
            notify_rust::Urgency::Normal,
        );
        self.emitir_estado("ocioso");
    }

    fn aviso(&mut self, mensagem: &str) {
        eprintln!("evervox-daemon: {mensagem}");
        self.notificar(mensagem, notify_rust::Urgency::Low);
    }
}

type MachineDoDaemon = Machine<
    DaemonFeedback,
    MicrofoneCpal,
    Box<dyn EngineSTT>,
    Box<dyn Limpeza>,
    EntregaClipboard,
    FocoGnome,
>;

struct DaemonService {
    machine: Mutex<MachineDoDaemon>,
    /// Resumo do Engine, da Limpeza e da Tradução resolvidos na
    /// inicialização (ver `resumir_engine`/`resumir_limpeza`/
    /// `resumir_traducao`), devolvido por [`DaemonService::status`].
    resumo_engine: String,
    resumo_limpeza: String,
    resumo_traducao: String,
}

#[interface(name = "com.evervox.Daemon1")]
impl DaemonService {
    /// Aciona o Toggle do Ditado. Retorna o novo estado: "ocioso" | "gravando".
    async fn toggle(&self) -> String {
        let resultado = {
            let mut machine = self.machine.lock().await;
            machine.toggle()
        };

        match resultado {
            Ok(ResultadoToggle::Gravando) => "gravando".to_string(),
            Ok(ResultadoToggle::Ocioso { audio }) => {
                salvar_gravacao(audio).await;
                "ocioso".to_string()
            }
            Err(erro) => {
                avisar_microfone_indisponivel(&erro).await;
                "ocioso".to_string()
            }
        }
    }

    /// Resumo de saúde do Daemon para `evervox status`: o Engine e a
    /// Limpeza resolvidos na inicialização. Responder já implica que o
    /// modelo/Engine terminou de carregar — o Daemon falha na inicialização
    /// e nunca chega a servir D-Bus se isso não acontecer (ver
    /// `preparar_engine`/`preparar_limpeza`).
    async fn status(&self) -> String {
        format!(
            "{}\n{}\n{}",
            self.resumo_engine, self.resumo_limpeza, self.resumo_traducao
        )
    }
}

/// Descreve o Engine resolvido pela config para `evervox status` (ver
/// [`DaemonService::status`]).
fn resumir_engine(config: &config::Config) -> String {
    match config.engine {
        EngineEscolhido::Local => format!(
            "engine: local (modelo '{}', carregado)",
            config.modelo_local
        ),
        EngineEscolhido::Cloud => "engine: cloud (OpenAI)".to_string(),
    }
}

/// Descreve a Limpeza resolvida pela config para `evervox status` (ver
/// [`DaemonService::status`]).
fn resumir_limpeza(config: &config::Config) -> String {
    if !config.limpeza.habilitada {
        return "limpeza: desligada".to_string();
    }
    let provedor = match config.limpeza.provedor {
        ProvedorLimpezaEscolhido::Openai => "openai",
        ProvedorLimpezaEscolhido::Anthropic => "anthropic",
    };
    format!(
        "limpeza: ligada (provedor '{provedor}', modelo '{}')",
        config.limpeza.modelo
    )
}

/// Descreve a Tradução resolvida pela config para `evervox status` (ver
/// [`DaemonService::status`]): ligada sempre que o Idioma de saída difere do
/// Idioma de entrada (ver [`traducao_ligada`]).
fn resumir_traducao(config: &config::Config) -> String {
    if !traducao_ligada(config) {
        return "tradução: desligada".to_string();
    }
    format!(
        "tradução: ligada ({} -> {})",
        config.idioma_entrada, config.idioma_saida
    )
}

/// Notifica algo que precisa da atenção do usuário fora do pipeline do
/// Ditado (falha ao salvar a Gravação, microfone indisponível, som/colar
/// indisponíveis na inicialização): loga no stderr e mostra uma notificação
/// com a mesma mensagem.
async fn avisar(mensagem: &str) {
    eprintln!("evervox-daemon: {mensagem}");
    let _ = Notification::new()
        .summary("EverVox")
        .body(mensagem)
        .show_async()
        .await;
}

async fn salvar_gravacao(audio: evervox_core::AudioGravado) {
    match wav::salvar(&audio) {
        Ok(caminho) => eprintln!("evervox-daemon: Gravação salva em {}", caminho.display()),
        Err(erro) => avisar(&format!("Falha ao salvar o áudio da Gravação: {erro}")).await,
    }
}

async fn avisar_microfone_indisponivel(erro: &ErroMicrofone) {
    avisar(&format!("Não foi possível iniciar a gravação: {erro}")).await;
}

/// Verifica se o tocador de som do freedesktop sound theme está no PATH.
/// Sem ele o Daemon segue funcionando, só sem o beep de feedback do Toggle.
fn canberra_disponivel() -> bool {
    Command::new("which")
        .arg("canberra-gtk-play")
        .output()
        .map(|saida| saida.status.success())
        .unwrap_or(false)
}

async fn avisar_beep_indisponivel() {
    avisar(
        "Som de feedback indisponível: instale o pacote com 'canberra-gtk-play' \
         (libcanberra) para ouvir o beep do Toggle.",
    )
    .await;
}

/// Prepara o Engine STT escolhido pela config — local (whisper.cpp) ou cloud
/// (API da OpenAI) — tudo bloqueante, feito uma única vez na inicialização
/// do Daemon. A escolha é estática: trocar de Engine exige reiniciar o
/// Daemon com a config atualizada (ver `CONTEXT.md`).
fn preparar_engine(config: &config::Config) -> anyhow::Result<Box<dyn EngineSTT>> {
    match config.engine {
        EngineEscolhido::Local => {
            let caminho_modelo = modelo::garantir(&config.modelo_local)?;
            eprintln!("evervox-daemon: carregando modelo whisper.cpp na memória...");
            let engine = EngineWhisper::carregar(
                &caminho_modelo,
                &config.idioma_entrada,
                &config.vocabulario,
            )?;
            eprintln!("evervox-daemon: modelo carregado, Engine local pronto.");
            Ok(Box::new(engine))
        }
        EngineEscolhido::Cloud => {
            let chave =
                evervox_segredo::carregar(engine_cloud::PROVEDOR_OPENAI)?.ok_or_else(|| {
                    anyhow::anyhow!(
                        "chave da OpenAI ausente: rode `evervox set-key {}`",
                        engine_cloud::PROVEDOR_OPENAI
                    )
                })?;
            eprintln!("evervox-daemon: Engine cloud (OpenAI) pronto.");
            Ok(Box::new(EngineCloud::nova(
                chave,
                &config.idioma_entrada,
                &config.vocabulario,
            )))
        }
    }
}

/// A Tradução está ligada quando o Idioma de saída difere do Idioma de
/// entrada (ver `CONTEXT.md`): não há flag própria de liga/desliga, o par de
/// idiomas já é a fonte da verdade.
fn traducao_ligada(config: &config::Config) -> bool {
    config.idioma_entrada != config.idioma_saida
}

fn contexto_limpeza(config: &config::Config) -> limpeza::ContextoLimpeza {
    limpeza::ContextoLimpeza {
        instrucoes: config.limpeza.instrucoes.clone(),
        vocabulario: config.vocabulario.clone(),
        pontuacao_falada: config.limpeza.pontuacao_falada,
    }
}

/// Decide o que a chamada de LLM desta invocação deve fazer a partir da
/// combinação Limpeza/Tradução ligadas na config — nunca chamada com as duas
/// desligadas (ver [`preparar_limpeza`], que usa [`limpeza::LimpezaDesativada`]
/// nesse caso sem exigir chave de API).
fn instrucao_llm(config: &config::Config, limpar: bool, traduzir: bool) -> limpeza::Instrucao {
    match (limpar, traduzir) {
        (true, false) => limpeza::Instrucao::Limpar(contexto_limpeza(config)),
        (false, true) => limpeza::Instrucao::Traduzir {
            idioma_saida: config.idioma_saida.clone(),
        },
        (true, true) => limpeza::Instrucao::LimparETraduzir {
            contexto: contexto_limpeza(config),
            idioma_saida: config.idioma_saida.clone(),
        },
        (false, false) => unreachable!("preparar_limpeza já devolveu LimpezaDesativada"),
    }
}

/// Prepara a Limpeza e/ou a Tradução escolhidas pela config — desligadas,
/// OpenAI ou Anthropic — tudo bloqueante, feito uma única vez na
/// inicialização do Daemon, como o Engine (ver [`preparar_engine`]). Limpeza
/// e Tradução são independentes (ver `CONTEXT.md`), mas quando ambas estão
/// ligadas compartilham uma única chamada de LLM (ver ADR 0003 e
/// [`limpeza::Instrucao`]), usando o provedor/modelo/timeout configurados
/// para a Limpeza. Com as duas desligadas, nenhuma chave de API é exigida: o
/// núcleo nem chega a chamar a Limpeza (ver [`evervox_core::LimpezaExecucao`]).
fn preparar_limpeza(
    config: &config::Config,
    timeout: std::time::Duration,
) -> anyhow::Result<Box<dyn Limpeza>> {
    let limpar = config.limpeza.habilitada;
    let traduzir = traducao_ligada(config);

    if !limpar && !traduzir {
        return Ok(Box::new(limpeza::LimpezaDesativada));
    }

    let instrucao = instrucao_llm(config, limpar, traduzir);

    match config.limpeza.provedor {
        ProvedorLimpezaEscolhido::Openai => {
            let chave = evervox_segredo::carregar(limpeza::PROVEDOR_OPENAI)?.ok_or_else(|| {
                anyhow::anyhow!(
                    "chave da OpenAI ausente: rode `evervox set-key {}`",
                    limpeza::PROVEDOR_OPENAI
                )
            })?;
            eprintln!(
                "evervox-daemon: Limpeza/Tradução via OpenAI ({}) pronta.",
                config.limpeza.modelo
            );
            Ok(Box::new(limpeza::LimpezaOpenAI::nova(
                chave,
                &config.limpeza.modelo,
                &instrucao,
                timeout,
            )))
        }
        ProvedorLimpezaEscolhido::Anthropic => {
            let chave =
                evervox_segredo::carregar(limpeza::PROVEDOR_ANTHROPIC)?.ok_or_else(|| {
                    anyhow::anyhow!(
                        "chave da Anthropic ausente: rode `evervox set-key {}`",
                        limpeza::PROVEDOR_ANTHROPIC
                    )
                })?;
            eprintln!(
                "evervox-daemon: Limpeza/Tradução via Anthropic ({}) pronta.",
                config.limpeza.modelo
            );
            Ok(Box::new(limpeza::LimpezaAnthropic::nova(
                chave,
                &config.limpeza.modelo,
                &instrucao,
                timeout,
            )))
        }
    }
}

#[tokio::main]
async fn main() -> zbus::Result<()> {
    if !canberra_disponivel() {
        avisar_beep_indisponivel().await;
    }

    let config = config::carregar_ou_criar().unwrap_or_else(|erro| {
        eprintln!("evervox-daemon: falha fatal ao carregar a config: {erro}");
        std::process::exit(1);
    });
    eprintln!(
        "evervox-daemon: config carregada (idioma_entrada={}, idioma_saida={}, modelo={}, engine={:?})",
        config.idioma_entrada, config.idioma_saida, config.modelo_local, config.engine
    );

    let engine = preparar_engine(&config).unwrap_or_else(|erro| {
        eprintln!("evervox-daemon: falha fatal ao preparar o Engine: {erro}");
        std::process::exit(1);
    });

    let timeout_limpeza = std::time::Duration::from_millis(config.limpeza.timeout_ms);
    let limpeza = preparar_limpeza(&config, timeout_limpeza).unwrap_or_else(|erro| {
        eprintln!("evervox-daemon: falha fatal ao preparar a Limpeza: {erro}");
        std::process::exit(1);
    });
    let limpeza_config = LimpezaExecucao {
        habilitada: config.limpeza.habilitada || traducao_ligada(&config),
        timeout: timeout_limpeza,
    };

    let (entrega, aviso_colar_indisponivel) = EntregaClipboard::nova();
    if let Some(mensagem) = aviso_colar_indisponivel {
        avisar(&mensagem).await;
    }

    // `FocoGnome::nova`/`DaemonFeedback::nova` abrem `zbus::blocking::Connection`
    // (ver `foco.rs` e `DaemonFeedback::emitir_estado`), que por baixo dos
    // panos constrói e roda seu próprio runtime Tokio. Chamado direto aqui
    // dentro do `async fn main` (rodando *já* sobre um runtime Tokio), isso
    // entra em pânico com "Cannot start a runtime from within a runtime" —
    // por isso a construção roda numa thread da pool bloqueante, fora do
    // contexto do runtime externo.
    let terminais_conhecidos = config.terminais_conhecidos.clone();
    let (foco, feedback) = tokio::task::spawn_blocking(move || {
        (
            FocoGnome::nova(terminais_conhecidos),
            DaemonFeedback::nova(),
        )
    })
    .await
    .expect("thread de inicialização do Foco/Feedback não deveria falhar");
    let resumo_engine = resumir_engine(&config);
    let resumo_limpeza = resumir_limpeza(&config);
    let resumo_traducao = resumir_traducao(&config);

    let service = DaemonService {
        machine: Mutex::new(Machine::new(
            feedback,
            MicrofoneCpal::default(),
            engine,
            limpeza,
            limpeza_config,
            entrega,
            foco,
        )),
        resumo_engine,
        resumo_limpeza,
        resumo_traducao,
    };

    let connection = connection::Builder::session()?
        .serve_at(dbus::OBJECT_PATH, service)?
        .build()
        .await?;

    connection
        .request_name(dbus::SERVICE_NAME)
        .await
        .map_err(|erro| {
            eprintln!(
                "evervox-daemon: não foi possível registrar '{}' no D-Bus \
                 (já há um daemon rodando?): {erro}",
                dbus::SERVICE_NAME
            );
            erro
        })?;

    eprintln!(
        "evervox-daemon: pronto em {} ({}).",
        dbus::OBJECT_PATH,
        dbus::INTERFACE_NAME
    );
    std::future::pending::<()>().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resumo_curto_fica_intacto() {
        assert_eq!(resumir_para_notificacao("oi mundo"), "oi mundo");
    }

    #[test]
    fn resumo_longo_e_truncado_com_elipse() {
        let texto = "a".repeat(TAMANHO_MAXIMO_NA_NOTIFICACAO + 10);
        let resumo = resumir_para_notificacao(&texto);
        assert_eq!(
            resumo.chars().count(),
            TAMANHO_MAXIMO_NA_NOTIFICACAO + 1 // +1 pelo caractere de elipse
        );
        assert!(resumo.ends_with('…'));
    }

    fn config_com_idiomas(entrada: &str, saida: &str) -> config::Config {
        config::Config {
            idioma_entrada: entrada.to_string(),
            idioma_saida: saida.to_string(),
            ..config::Config::default()
        }
    }

    #[test]
    fn traducao_desligada_quando_idiomas_sao_iguais() {
        assert!(!traducao_ligada(&config_com_idiomas("pt", "pt")));
    }

    #[test]
    fn traducao_ligada_quando_idiomas_diferem() {
        assert!(traducao_ligada(&config_com_idiomas("pt", "en")));
    }

    #[test]
    fn resumo_de_traducao_desligada() {
        assert_eq!(
            resumir_traducao(&config_com_idiomas("pt", "pt")),
            "tradução: desligada"
        );
    }

    #[test]
    fn resumo_de_traducao_ligada_mostra_o_par_de_idiomas() {
        assert_eq!(
            resumir_traducao(&config_com_idiomas("pt", "en")),
            "tradução: ligada (pt -> en)"
        );
    }

    #[test]
    fn instrucao_traduzir_quando_so_a_traducao_esta_ligada() {
        let config = config_com_idiomas("pt", "en");

        let instrucao = instrucao_llm(&config, false, true);

        assert_eq!(
            instrucao,
            limpeza::Instrucao::Traduzir {
                idioma_saida: "en".to_string()
            }
        );
    }

    #[test]
    fn instrucao_limpar_quando_so_a_limpeza_esta_ligada() {
        let config = config_com_idiomas("pt", "pt");

        let instrucao = instrucao_llm(&config, true, false);

        assert_eq!(
            instrucao,
            limpeza::Instrucao::Limpar(contexto_limpeza(&config))
        );
    }

    #[test]
    fn instrucao_funde_limpeza_e_traducao_quando_ambas_ligadas() {
        let config = config_com_idiomas("pt", "en");

        let instrucao = instrucao_llm(&config, true, true);

        assert_eq!(
            instrucao,
            limpeza::Instrucao::LimparETraduzir {
                contexto: contexto_limpeza(&config),
                idioma_saida: "en".to_string(),
            }
        );
    }
}
