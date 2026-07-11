//! Núcleo do Ditado: a máquina de estados acionada pelo Toggle.
//!
//! Ocioso <-> Gravando é síncrono e controla o Toggle. Ao encerrar a
//! Gravação, o Processando (Engine STT + Entrega) roda em segundo plano,
//! numa thread própria: o núcleo volta a Ocioso imediatamente, então um novo
//! Toggle pode iniciar outra Gravação sem esperar o Ditado anterior terminar
//! seu curso. A Limpeza (próximo ticket) entra nesse mesmo pipeline.

use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

/// Taxa de amostragem exigida do áudio do Ditado: formato adequado para STT.
pub const TAXA_AMOSTRAGEM_HZ: u32 = 16_000;

pub mod dbus {
    //! Endereço D-Bus compartilhado entre o Daemon e a CLI.
    pub const SERVICE_NAME: &str = "com.evervox.Daemon";
    pub const OBJECT_PATH: &str = "/com/evervox/Daemon";
    pub const INTERFACE_NAME: &str = "com.evervox.Daemon1";
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DitadoState {
    Ocioso,
    Gravando,
}

impl DitadoState {
    pub fn as_str(&self) -> &'static str {
        match self {
            DitadoState::Ocioso => "ocioso",
            DitadoState::Gravando => "gravando",
        }
    }
}

/// Porta de feedback sensorial e de notificação do Ditado.
/// Implementações reais ficam no Daemon; testes usam um fake.
///
/// Os eventos de Processando chegam de uma thread em segundo plano (nunca da
/// thread que chamou [`Machine::toggle`]), então implementações precisam ser
/// seguras para uso concorrente com elas mesmas (mas nunca chamadas em
/// paralelo: o núcleo serializa as chamadas).
pub trait Feedback: Send {
    fn iniciou_gravacao(&mut self);
    fn encerrou_gravacao(&mut self);
    /// O Ditado foi transcrito e o texto foi colado com sucesso no app focado
    /// (o clipboard anterior já foi restaurado, ou a tentativa de restaurar
    /// falhou — nesse caso um [`Feedback::aviso`] separado é emitido).
    fn concluiu_ditado(&mut self, texto: &str);
    /// A Gravação não continha fala detectável: nada foi entregue.
    fn ditado_silencioso(&mut self);
    /// O colar automático falhou; a Transcrição permanece no clipboard como
    /// fallback manual (o usuário pode colar com Ctrl+V).
    fn ditado_no_clipboard_sem_colar(&mut self, texto: &str);
    /// O Engine ou a Entrega falharam antes de colar; nada chegou ao usuário.
    fn falha_ditado(&mut self, mensagem: &str);
    /// Aviso não crítico do Ditado: o Ditado já foi concluído (ou falhou por
    /// outro motivo já reportado), mas algo secundário não saiu como
    /// esperado — ex.: não foi possível restaurar o clipboard anterior.
    fn aviso(&mut self, mensagem: &str);
}

/// O áudio completo de uma Gravação, pronto para a próxima etapa do Ditado.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioGravado {
    pub amostras: Vec<i16>,
    pub taxa_amostragem_hz: u32,
}

/// Falha ao acessar o microfone para iniciar ou manter a Gravação.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErroMicrofone(pub String);

impl std::fmt::Display for ErroMicrofone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "microfone indisponível: {}", self.0)
    }
}

impl std::error::Error for ErroMicrofone {}

/// Falha do Engine STT ao transcrever a Gravação.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErroEngine(pub String);

impl std::fmt::Display for ErroEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "falha na transcrição: {}", self.0)
    }
}

impl std::error::Error for ErroEngine {}

/// Falha ao entregar a Transcrição.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErroEntrega(pub String);

impl std::fmt::Display for ErroEntrega {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "falha na entrega: {}", self.0)
    }
}

impl std::error::Error for ErroEntrega {}

/// Callback que recebe cada bloco de amostras produzido por uma [`FonteDeAudio`].
pub type CallbackAmostras = Box<dyn FnMut(&[i16]) + Send>;

/// Porta para o dispositivo de captura de áudio (o microfone).
/// Implementações reais ficam no Daemon (via `cpal`); testes usam um fake.
pub trait FonteDeAudio {
    /// Abre o dispositivo e inicia a captura. Cada bloco de amostras chega
    /// via `on_amostras`, em taxa e formato já normalizados para
    /// [`TAXA_AMOSTRAGEM_HZ`] mono, até `encerrar` ser chamado.
    fn iniciar(&mut self, on_amostras: CallbackAmostras) -> Result<(), ErroMicrofone>;

    /// Encerra a captura e libera o dispositivo.
    fn encerrar(&mut self);
}

/// Porta do Engine STT: transcreve o áudio de uma Gravação completa.
/// Implementações reais (whisper.cpp local, API cloud) ficam no Daemon;
/// testes usam um fake.
pub trait EngineSTT: Send {
    fn transcrever(&mut self, audio: &AudioGravado) -> Result<String, ErroEngine>;
}

/// Porta de Entrega (ver ADR 0001): entrega a Transcrição (crua, ou limpa no
/// futuro) ao app focado via clipboard + colar simulado, restaurando o
/// clipboard anterior depois. O núcleo orquestra os quatro passos, sempre
/// nessa ordem — salvar, copiar, colar, restaurar — para que o comportamento
/// externo (o que foi entregue e a restauração do clipboard) seja
/// verificável na costura com uma Entrega fake, sem depender de detalhes do
/// adaptador real. Implementações reais (`wl-copy`/`wl-paste` + `uinput`)
/// ficam no Daemon.
pub trait Entrega: Send {
    /// Retrato do clipboard, salvo por [`salvar_clipboard`] para ser
    /// devolvido a [`restaurar_clipboard`]. O núcleo não interpreta o
    /// conteúdo — cada Entrega escolhe seu próprio formato (texto, imagem,
    /// ambos, ou vazio).
    type ClipboardSalvo: Send;

    /// Salva o clipboard atual, antes de sobrescrevê-lo com a Transcrição.
    fn salvar_clipboard(&mut self) -> Result<Self::ClipboardSalvo, ErroEntrega>;
    /// Copia o texto da Transcrição para o clipboard.
    fn copiar(&mut self, texto: &str) -> Result<(), ErroEntrega>;
    /// Simula o atalho de colar no app focado (ver [`Atalho`]).
    fn colar(&mut self, atalho: Atalho) -> Result<(), ErroEntrega>;
    /// Restaura o clipboard salvo por [`salvar_clipboard`].
    fn restaurar_clipboard(&mut self, salvo: Self::ClipboardSalvo) -> Result<(), ErroEntrega>;
}

/// Atalho de colar a simular na Entrega, decidido a partir do app focado
/// (ver [`Foco`]). Terminais não recebem `Ctrl+V` (costuma abrir um menu ou
/// nada acontece); usam `Ctrl+Shift+V`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Atalho {
    /// `Ctrl+V`, para qualquer app que não seja um terminal.
    Padrao,
    /// `Ctrl+Shift+V`, para terminais.
    Terminal,
}

/// Porta de Foco (ver ADR 0001): decide o [`Atalho`] de colar a partir do app
/// focado no momento da Entrega. A implementação real consulta a extensão
/// GNOME Shell via D-Bus e compara contra a lista de terminais conhecidos da
/// config; se a extensão estiver ausente ou a consulta falhar, degrada para
/// [`Atalho::Padrao`] sem erro — o método nunca falha. Fica no Daemon;
/// testes usam um fake fixo.
pub trait Foco: Send {
    /// Decide o [`Atalho`] de colar a usar agora, a partir do app focado.
    fn atalho_de_colar(&mut self) -> Atalho;
}

/// Acumula as amostras produzidas por uma [`FonteDeAudio`] durante uma
/// Gravação, entregando o áudio completo quando ela é encerrada.
struct Gravador<G: FonteDeAudio> {
    fonte: G,
    buffer: Arc<Mutex<Vec<i16>>>,
}

impl<G: FonteDeAudio> Gravador<G> {
    fn new(fonte: G) -> Self {
        Self {
            fonte,
            buffer: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn iniciar(&mut self) -> Result<(), ErroMicrofone> {
        self.buffer.lock().unwrap().clear();
        let buffer = Arc::clone(&self.buffer);
        self.fonte.iniciar(Box::new(move |amostras| {
            buffer.lock().unwrap().extend_from_slice(amostras);
        }))
    }

    fn encerrar(&mut self) -> AudioGravado {
        self.fonte.encerrar();
        let amostras = std::mem::take(&mut *self.buffer.lock().unwrap());
        AudioGravado {
            amostras,
            taxa_amostragem_hz: TAXA_AMOSTRAGEM_HZ,
        }
    }
}

/// Resultado de um Toggle bem-sucedido.
#[derive(Debug, Clone, PartialEq)]
pub enum ResultadoToggle {
    Gravando,
    /// A Gravação encerrou; `audio` é o que foi capturado. O Processando
    /// (transcrição + Entrega) já foi iniciado em segundo plano.
    Ocioso {
        audio: AudioGravado,
    },
}

impl ResultadoToggle {
    pub fn estado(&self) -> DitadoState {
        match self {
            ResultadoToggle::Gravando => DitadoState::Gravando,
            ResultadoToggle::Ocioso { .. } => DitadoState::Ocioso,
        }
    }
}

pub struct Machine<F: Feedback, G: FonteDeAudio, E: EngineSTT, D: Entrega, O: Foco> {
    state: DitadoState,
    feedback: Arc<Mutex<F>>,
    gravador: Gravador<G>,
    engine: Arc<Mutex<E>>,
    entrega: Arc<Mutex<D>>,
    foco: Arc<Mutex<O>>,
    processamentos: Vec<JoinHandle<()>>,
}

impl<F, G, E, D, O> Machine<F, G, E, D, O>
where
    F: Feedback + 'static,
    G: FonteDeAudio,
    E: EngineSTT + 'static,
    D: Entrega + 'static,
    O: Foco + 'static,
{
    pub fn new(feedback: F, fonte_de_audio: G, engine: E, entrega: D, foco: O) -> Self {
        Self {
            state: DitadoState::Ocioso,
            feedback: Arc::new(Mutex::new(feedback)),
            gravador: Gravador::new(fonte_de_audio),
            engine: Arc::new(Mutex::new(engine)),
            entrega: Arc::new(Mutex::new(entrega)),
            foco: Arc::new(Mutex::new(foco)),
            processamentos: Vec::new(),
        }
    }

    pub fn state(&self) -> DitadoState {
        self.state
    }

    /// Aciona o Toggle: Ocioso -> Gravando ou Gravando -> Ocioso.
    ///
    /// Se o microfone estiver indisponível ao iniciar, o estado permanece
    /// Ocioso e nenhum feedback de início é disparado. Ao encerrar, o
    /// Processando da Gravação é despachado para uma thread em segundo
    /// plano; o núcleo já está livre para um novo Toggle antes dele terminar.
    pub fn toggle(&mut self) -> Result<ResultadoToggle, ErroMicrofone> {
        match self.state {
            DitadoState::Ocioso => {
                self.gravador.iniciar()?;
                self.state = DitadoState::Gravando;
                self.feedback.lock().unwrap().iniciou_gravacao();
                Ok(ResultadoToggle::Gravando)
            }
            DitadoState::Gravando => {
                let audio = self.gravador.encerrar();
                self.state = DitadoState::Ocioso;
                self.feedback.lock().unwrap().encerrou_gravacao();
                self.despachar_processamento(audio.clone());
                Ok(ResultadoToggle::Ocioso { audio })
            }
        }
    }

    /// Espera todos os Processandos despachados até agora terminarem seu
    /// curso. Usado por testes para tornar a conclusão determinística; o
    /// Daemon não precisa chamar isso — o Ditado entrega assincronamente.
    pub fn aguardar_processamentos(&mut self) {
        for handle in self.processamentos.drain(..) {
            let _ = handle.join();
        }
    }

    fn despachar_processamento(&mut self, audio: AudioGravado) {
        let engine = Arc::clone(&self.engine);
        let entrega = Arc::clone(&self.entrega);
        let foco = Arc::clone(&self.foco);
        let feedback = Arc::clone(&self.feedback);

        let handle = thread::spawn(move || {
            let transcricao = engine.lock().unwrap().transcrever(&audio);
            match transcricao {
                Ok(texto) if texto.trim().is_empty() => {
                    feedback.lock().unwrap().ditado_silencioso();
                }
                Ok(texto) => {
                    let texto = texto.trim();
                    let atalho = foco.lock().unwrap().atalho_de_colar();
                    let resultado =
                        entregar_com_colar_simulado(&mut *entrega.lock().unwrap(), texto, atalho);
                    let mut feedback = feedback.lock().unwrap();
                    match resultado {
                        Ok(None) => feedback.concluiu_ditado(texto),
                        Ok(Some(erro_ao_restaurar)) => {
                            feedback.concluiu_ditado(texto);
                            feedback.aviso(&format!(
                                "não foi possível restaurar o clipboard anterior: {erro_ao_restaurar}"
                            ));
                        }
                        Err(FalhaEntrega::AoColar) => feedback.ditado_no_clipboard_sem_colar(texto),
                        Err(FalhaEntrega::Outra(erro)) => feedback.falha_ditado(&erro.to_string()),
                    }
                }
                Err(erro) => feedback.lock().unwrap().falha_ditado(&erro.to_string()),
            }
        });
        self.processamentos.push(handle);
    }
}

/// Falha da Entrega antes de colar (nada foi entregue) ou ao colar
/// especificamente (o texto já está no clipboard como fallback manual — ver
/// [`entregar_com_colar_simulado`]).
enum FalhaEntrega {
    AoColar,
    Outra(ErroEntrega),
}

/// Executa a Entrega na ordem exigida pelo ADR 0001: salva o clipboard
/// atual, copia a Transcrição, simula o colar e restaura o clipboard salvo.
///
/// Se o colar falhar, a Transcrição permanece no clipboard (não restaura) —
/// é o fallback manual do usuário. Se só a restauração falhar (o colar já
/// tinha funcionado), o Ditado é considerado concluído; a falha ao restaurar
/// volta como `Ok(Some(erro))` para o chamador emitir um aviso não crítico.
fn entregar_com_colar_simulado<D: Entrega>(
    entrega: &mut D,
    texto: &str,
    atalho: Atalho,
) -> Result<Option<ErroEntrega>, FalhaEntrega> {
    let salvo = entrega.salvar_clipboard().map_err(FalhaEntrega::Outra)?;
    entrega.copiar(texto).map_err(FalhaEntrega::Outra)?;
    entrega.colar(atalho).map_err(|_| FalhaEntrega::AoColar)?;
    Ok(entrega.restaurar_clipboard(salvo).err())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Event {
        Iniciou,
        Encerrou,
        Concluiu(String),
        Silencioso,
        SemColar(String),
        Falha(String),
        Aviso(String),
    }

    #[derive(Default, Clone)]
    struct FakeFeedback {
        events: Arc<Mutex<Vec<Event>>>,
    }

    impl FakeFeedback {
        fn events(&self) -> Vec<Event> {
            self.events.lock().unwrap().clone()
        }
    }

    impl Feedback for FakeFeedback {
        fn iniciou_gravacao(&mut self) {
            self.events.lock().unwrap().push(Event::Iniciou);
        }

        fn encerrou_gravacao(&mut self) {
            self.events.lock().unwrap().push(Event::Encerrou);
        }

        fn concluiu_ditado(&mut self, texto: &str) {
            self.events
                .lock()
                .unwrap()
                .push(Event::Concluiu(texto.to_string()));
        }

        fn ditado_silencioso(&mut self) {
            self.events.lock().unwrap().push(Event::Silencioso);
        }

        fn ditado_no_clipboard_sem_colar(&mut self, texto: &str) {
            self.events
                .lock()
                .unwrap()
                .push(Event::SemColar(texto.to_string()));
        }

        fn falha_ditado(&mut self, mensagem: &str) {
            self.events
                .lock()
                .unwrap()
                .push(Event::Falha(mensagem.to_string()));
        }

        fn aviso(&mut self, mensagem: &str) {
            self.events
                .lock()
                .unwrap()
                .push(Event::Aviso(mensagem.to_string()));
        }
    }

    /// Fonte de áudio fake: emite blocos de amostras pré-programados assim
    /// que `iniciar` é chamado, simulando o que o dispositivo real produziria
    /// ao longo da Gravação. Pode ser configurada para falhar, simulando um
    /// microfone indisponível.
    #[derive(Default)]
    struct FakeFonteDeAudio {
        blocos: Vec<Vec<i16>>,
        falha: Option<String>,
    }

    impl FonteDeAudio for FakeFonteDeAudio {
        fn iniciar(
            &mut self,
            mut on_amostras: Box<dyn FnMut(&[i16]) + Send>,
        ) -> Result<(), ErroMicrofone> {
            if let Some(motivo) = &self.falha {
                return Err(ErroMicrofone(motivo.clone()));
            }
            for bloco in &self.blocos {
                on_amostras(bloco);
            }
            Ok(())
        }

        fn encerrar(&mut self) {}
    }

    /// Engine fake que retorna uma transcrição fixa (ou uma falha) de imediato.
    #[derive(Clone)]
    struct FakeEngine {
        resultado: Result<String, ErroEngine>,
    }

    impl FakeEngine {
        fn sucesso(texto: &str) -> Self {
            Self {
                resultado: Ok(texto.to_string()),
            }
        }

        fn falha(mensagem: &str) -> Self {
            Self {
                resultado: Err(ErroEngine(mensagem.to_string())),
            }
        }
    }

    impl EngineSTT for FakeEngine {
        fn transcrever(&mut self, _audio: &AudioGravado) -> Result<String, ErroEngine> {
            self.resultado.clone()
        }
    }

    /// Engine fake que bloqueia o Processando até o teste liberar,
    /// sinalizando quando chegou a ser chamado. Usado para testar o
    /// comportamento do núcleo enquanto um Processando está em curso.
    struct EngineBloqueante {
        chegou_tx: mpsc::Sender<()>,
        liberar_rx: mpsc::Receiver<()>,
        texto: String,
    }

    impl EngineSTT for EngineBloqueante {
        fn transcrever(&mut self, _audio: &AudioGravado) -> Result<String, ErroEngine> {
            let _ = self.chegou_tx.send(());
            let _ = self.liberar_rx.recv();
            Ok(self.texto.clone())
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum EtapaEntrega {
        Salvar,
        Copiar,
        Colar,
        Restaurar,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum EntregaEvento {
        Salvou,
        Copiou(String),
        Colou(Atalho),
        Restaurou(String),
    }

    /// Entrega fake: registra a ordem de chamadas (salvar/copiar/colar/
    /// restaurar) e pode ser configurada para falhar numa etapa específica.
    #[derive(Default, Clone)]
    struct FakeEntrega {
        eventos: Arc<Mutex<Vec<EntregaEvento>>>,
        clipboard_anterior: String,
        falhar_em: Option<EtapaEntrega>,
    }

    impl FakeEntrega {
        fn com_clipboard_anterior(texto: &str) -> Self {
            Self {
                clipboard_anterior: texto.to_string(),
                ..Default::default()
            }
        }

        fn que_falha_em(etapa: EtapaEntrega) -> Self {
            Self {
                falhar_em: Some(etapa),
                ..Default::default()
            }
        }

        fn eventos(&self) -> Vec<EntregaEvento> {
            self.eventos.lock().unwrap().clone()
        }
    }

    impl Entrega for FakeEntrega {
        type ClipboardSalvo = String;

        fn salvar_clipboard(&mut self) -> Result<String, ErroEntrega> {
            if self.falhar_em == Some(EtapaEntrega::Salvar) {
                return Err(ErroEntrega("falhou ao salvar o clipboard".to_string()));
            }
            self.eventos.lock().unwrap().push(EntregaEvento::Salvou);
            Ok(self.clipboard_anterior.clone())
        }

        fn copiar(&mut self, texto: &str) -> Result<(), ErroEntrega> {
            if self.falhar_em == Some(EtapaEntrega::Copiar) {
                return Err(ErroEntrega("falhou ao copiar para o clipboard".to_string()));
            }
            self.eventos
                .lock()
                .unwrap()
                .push(EntregaEvento::Copiou(texto.to_string()));
            Ok(())
        }

        fn colar(&mut self, atalho: Atalho) -> Result<(), ErroEntrega> {
            if self.falhar_em == Some(EtapaEntrega::Colar) {
                return Err(ErroEntrega("falhou ao simular o colar".to_string()));
            }
            self.eventos
                .lock()
                .unwrap()
                .push(EntregaEvento::Colou(atalho));
            Ok(())
        }

        fn restaurar_clipboard(&mut self, salvo: String) -> Result<(), ErroEntrega> {
            if self.falhar_em == Some(EtapaEntrega::Restaurar) {
                return Err(ErroEntrega(
                    "falhou ao restaurar o clipboard anterior".to_string(),
                ));
            }
            self.eventos
                .lock()
                .unwrap()
                .push(EntregaEvento::Restaurou(salvo));
            Ok(())
        }
    }

    /// Foco fake: devolve sempre o mesmo [`Atalho`], simulando um app
    /// terminal, não-terminal, ou a extensão GNOME indisponível (que também
    /// degrada para [`Atalho::Padrao`], como a implementação real).
    #[derive(Clone)]
    struct FakeFoco {
        atalho: Atalho,
    }

    impl FakeFoco {
        fn terminal() -> Self {
            Self {
                atalho: Atalho::Terminal,
            }
        }

        fn nao_terminal() -> Self {
            Self {
                atalho: Atalho::Padrao,
            }
        }

        fn indisponivel() -> Self {
            Self {
                atalho: Atalho::Padrao,
            }
        }
    }

    impl Default for FakeFoco {
        fn default() -> Self {
            Self::nao_terminal()
        }
    }

    impl Foco for FakeFoco {
        fn atalho_de_colar(&mut self) -> Atalho {
            self.atalho
        }
    }

    fn nova_machine(
        fonte: FakeFonteDeAudio,
        engine: FakeEngine,
        entrega: FakeEntrega,
        feedback: FakeFeedback,
    ) -> Machine<FakeFeedback, FakeFonteDeAudio, FakeEngine, FakeEntrega, FakeFoco> {
        nova_machine_com_foco(fonte, engine, entrega, feedback, FakeFoco::default())
    }

    fn nova_machine_com_foco(
        fonte: FakeFonteDeAudio,
        engine: FakeEngine,
        entrega: FakeEntrega,
        feedback: FakeFeedback,
        foco: FakeFoco,
    ) -> Machine<FakeFeedback, FakeFonteDeAudio, FakeEngine, FakeEntrega, FakeFoco> {
        Machine::new(feedback, fonte, engine, entrega, foco)
    }

    fn nova_machine_padrao(
        fonte: FakeFonteDeAudio,
    ) -> Machine<FakeFeedback, FakeFonteDeAudio, FakeEngine, FakeEntrega, FakeFoco> {
        nova_machine(
            fonte,
            FakeEngine::sucesso(""),
            FakeEntrega::default(),
            FakeFeedback::default(),
        )
    }

    #[test]
    fn comeca_ocioso() {
        let machine = nova_machine_padrao(FakeFonteDeAudio::default());
        assert_eq!(machine.state(), DitadoState::Ocioso);
    }

    #[test]
    fn primeiro_toggle_inicia_gravacao_e_notifica() {
        let feedback = FakeFeedback::default();
        let mut machine = nova_machine(
            FakeFonteDeAudio::default(),
            FakeEngine::sucesso(""),
            FakeEntrega::default(),
            feedback.clone(),
        );

        let resultado = machine.toggle().unwrap();

        assert_eq!(resultado, ResultadoToggle::Gravando);
        assert_eq!(feedback.events(), vec![Event::Iniciou]);
    }

    #[test]
    fn toggle_duplo_alterna_estados_e_volta_ao_ocioso() {
        let feedback = FakeFeedback::default();
        let mut machine = nova_machine(
            FakeFonteDeAudio::default(),
            FakeEngine::sucesso(""),
            FakeEntrega::default(),
            feedback.clone(),
        );

        let apos_primeiro = machine.toggle().unwrap();
        let apos_segundo = machine.toggle().unwrap();
        machine.aguardar_processamentos();

        assert_eq!(apos_primeiro.estado(), DitadoState::Gravando);
        assert_eq!(apos_segundo.estado(), DitadoState::Ocioso);
        assert_eq!(
            feedback.events(),
            vec![Event::Iniciou, Event::Encerrou, Event::Silencioso]
        );
    }

    #[test]
    fn tres_toggles_termina_gravando() {
        let mut machine = nova_machine_padrao(FakeFonteDeAudio::default());

        machine.toggle().unwrap();
        machine.toggle().unwrap();
        let terceiro = machine.toggle().unwrap();
        machine.aguardar_processamentos();

        assert_eq!(terceiro.estado(), DitadoState::Gravando);
    }

    #[test]
    fn ao_encerrar_o_audio_completo_chega_ao_engine() {
        let fonte = FakeFonteDeAudio {
            blocos: vec![vec![1, 2, 3], vec![4, 5]],
            falha: None,
        };
        let mut machine = nova_machine_padrao(fonte);

        machine.toggle().unwrap(); // inicia a gravação, a fonte emite os blocos
        let resultado = machine.toggle().unwrap(); // encerra

        match resultado {
            ResultadoToggle::Ocioso { audio } => {
                assert_eq!(audio.amostras, vec![1, 2, 3, 4, 5]);
                assert_eq!(audio.taxa_amostragem_hz, TAXA_AMOSTRAGEM_HZ);
            }
            other => panic!("esperava Ocioso com áudio, obteve {other:?}"),
        }
        machine.aguardar_processamentos();
    }

    #[test]
    fn falha_de_microfone_mantem_ocioso_e_nao_notifica() {
        let fonte = FakeFonteDeAudio {
            blocos: vec![],
            falha: Some("dispositivo indisponível".to_string()),
        };
        let feedback = FakeFeedback::default();
        let mut machine = nova_machine(
            fonte,
            FakeEngine::sucesso(""),
            FakeEntrega::default(),
            feedback.clone(),
        );

        let erro = machine.toggle().unwrap_err();

        assert_eq!(erro, ErroMicrofone("dispositivo indisponível".to_string()));
        assert_eq!(machine.state(), DitadoState::Ocioso);
        assert!(feedback.events().is_empty());
    }

    #[test]
    fn fluxo_feliz_salva_copia_cola_e_restaura_nessa_ordem() {
        let feedback = FakeFeedback::default();
        let entrega = FakeEntrega::com_clipboard_anterior("conteúdo antigo");
        let mut machine = nova_machine(
            FakeFonteDeAudio::default(),
            FakeEngine::sucesso("oi mundo"),
            entrega.clone(),
            feedback.clone(),
        );

        machine.toggle().unwrap();
        machine.toggle().unwrap();
        machine.aguardar_processamentos();

        assert_eq!(
            entrega.eventos(),
            vec![
                EntregaEvento::Salvou,
                EntregaEvento::Copiou("oi mundo".to_string()),
                EntregaEvento::Colou(Atalho::Padrao),
                EntregaEvento::Restaurou("conteúdo antigo".to_string()),
            ]
        );
        assert_eq!(
            feedback.events(),
            vec![
                Event::Iniciou,
                Event::Encerrou,
                Event::Concluiu("oi mundo".to_string())
            ]
        );
    }

    #[test]
    fn foco_terminal_cola_com_atalho_de_terminal() {
        let entrega = FakeEntrega::default();
        let mut machine = nova_machine_com_foco(
            FakeFonteDeAudio::default(),
            FakeEngine::sucesso("oi mundo"),
            entrega.clone(),
            FakeFeedback::default(),
            FakeFoco::terminal(),
        );

        machine.toggle().unwrap();
        machine.toggle().unwrap();
        machine.aguardar_processamentos();

        assert!(entrega
            .eventos()
            .contains(&EntregaEvento::Colou(Atalho::Terminal)));
    }

    #[test]
    fn foco_nao_terminal_cola_com_atalho_padrao() {
        let entrega = FakeEntrega::default();
        let mut machine = nova_machine_com_foco(
            FakeFonteDeAudio::default(),
            FakeEngine::sucesso("oi mundo"),
            entrega.clone(),
            FakeFeedback::default(),
            FakeFoco::nao_terminal(),
        );

        machine.toggle().unwrap();
        machine.toggle().unwrap();
        machine.aguardar_processamentos();

        assert!(entrega
            .eventos()
            .contains(&EntregaEvento::Colou(Atalho::Padrao)));
    }

    #[test]
    fn foco_indisponivel_degrada_para_atalho_padrao() {
        let entrega = FakeEntrega::default();
        let mut machine = nova_machine_com_foco(
            FakeFonteDeAudio::default(),
            FakeEngine::sucesso("oi mundo"),
            entrega.clone(),
            FakeFeedback::default(),
            FakeFoco::indisponivel(),
        );

        machine.toggle().unwrap();
        machine.toggle().unwrap();
        machine.aguardar_processamentos();

        assert!(entrega
            .eventos()
            .contains(&EntregaEvento::Colou(Atalho::Padrao)));
    }

    #[test]
    fn ditado_silencioso_nao_toca_a_entrega() {
        let feedback = FakeFeedback::default();
        let entrega = FakeEntrega::default();
        let mut machine = nova_machine(
            FakeFonteDeAudio::default(),
            FakeEngine::sucesso("   "),
            entrega.clone(),
            feedback.clone(),
        );

        machine.toggle().unwrap();
        machine.toggle().unwrap();
        machine.aguardar_processamentos();

        assert!(entrega.eventos().is_empty());
        assert_eq!(
            feedback.events(),
            vec![Event::Iniciou, Event::Encerrou, Event::Silencioso]
        );
    }

    #[test]
    fn falha_do_engine_notifica_e_nao_toca_a_entrega() {
        let feedback = FakeFeedback::default();
        let entrega = FakeEntrega::default();
        let mut machine = nova_machine(
            FakeFonteDeAudio::default(),
            FakeEngine::falha("modelo indisponível"),
            entrega.clone(),
            feedback.clone(),
        );

        machine.toggle().unwrap();
        machine.toggle().unwrap();
        machine.aguardar_processamentos();

        assert!(entrega.eventos().is_empty());
        assert_eq!(
            feedback.events(),
            vec![
                Event::Iniciou,
                Event::Encerrou,
                Event::Falha("falha na transcrição: modelo indisponível".to_string())
            ]
        );
    }

    #[test]
    fn falha_ao_salvar_o_clipboard_notifica_e_nao_copia_nada() {
        let feedback = FakeFeedback::default();
        let entrega = FakeEntrega::que_falha_em(EtapaEntrega::Salvar);
        let mut machine = nova_machine(
            FakeFonteDeAudio::default(),
            FakeEngine::sucesso("oi mundo"),
            entrega.clone(),
            feedback.clone(),
        );

        machine.toggle().unwrap();
        machine.toggle().unwrap();
        machine.aguardar_processamentos();

        assert!(entrega.eventos().is_empty());
        assert_eq!(
            feedback.events(),
            vec![
                Event::Iniciou,
                Event::Encerrou,
                Event::Falha("falha na entrega: falhou ao salvar o clipboard".to_string())
            ]
        );
    }

    #[test]
    fn falha_ao_copiar_notifica_e_nao_cola_nem_restaura() {
        let feedback = FakeFeedback::default();
        let entrega = FakeEntrega::que_falha_em(EtapaEntrega::Copiar);
        let mut machine = nova_machine(
            FakeFonteDeAudio::default(),
            FakeEngine::sucesso("oi mundo"),
            entrega.clone(),
            feedback.clone(),
        );

        machine.toggle().unwrap();
        machine.toggle().unwrap();
        machine.aguardar_processamentos();

        // Salvou o clipboard atual (para restaurar), mas a cópia falhou
        // antes de tocar em colar/restaurar: o clipboard original nunca foi
        // sobrescrito, então não há nada a restaurar.
        assert_eq!(entrega.eventos(), vec![EntregaEvento::Salvou]);
        assert_eq!(
            feedback.events(),
            vec![
                Event::Iniciou,
                Event::Encerrou,
                Event::Falha("falha na entrega: falhou ao copiar para o clipboard".to_string())
            ]
        );
    }

    #[test]
    fn falha_ao_colar_mantem_o_texto_no_clipboard_como_fallback() {
        let feedback = FakeFeedback::default();
        let entrega = FakeEntrega::que_falha_em(EtapaEntrega::Colar);
        let mut machine = nova_machine(
            FakeFonteDeAudio::default(),
            FakeEngine::sucesso("oi mundo"),
            entrega.clone(),
            feedback.clone(),
        );

        machine.toggle().unwrap();
        machine.toggle().unwrap();
        machine.aguardar_processamentos();

        // Salvou e copiou (o texto está no clipboard), mas não colou nem
        // restaurou: o clipboard fica com a Transcrição, não com o anterior.
        assert_eq!(
            entrega.eventos(),
            vec![
                EntregaEvento::Salvou,
                EntregaEvento::Copiou("oi mundo".to_string()),
            ]
        );
        assert_eq!(
            feedback.events(),
            vec![
                Event::Iniciou,
                Event::Encerrou,
                Event::SemColar("oi mundo".to_string())
            ]
        );
    }

    #[test]
    fn falha_ao_restaurar_conclui_o_ditado_e_emite_um_aviso() {
        let feedback = FakeFeedback::default();
        let entrega = FakeEntrega::que_falha_em(EtapaEntrega::Restaurar);
        let mut machine = nova_machine(
            FakeFonteDeAudio::default(),
            FakeEngine::sucesso("oi mundo"),
            entrega.clone(),
            feedback.clone(),
        );

        machine.toggle().unwrap();
        machine.toggle().unwrap();
        machine.aguardar_processamentos();

        assert_eq!(
            entrega.eventos(),
            vec![
                EntregaEvento::Salvou,
                EntregaEvento::Copiou("oi mundo".to_string()),
                EntregaEvento::Colou(Atalho::Padrao),
            ]
        );
        assert_eq!(
            feedback.events(),
            vec![
                Event::Iniciou,
                Event::Encerrou,
                Event::Concluiu("oi mundo".to_string()),
                Event::Aviso(
                    "não foi possível restaurar o clipboard anterior: \
                     falha na entrega: falhou ao restaurar o clipboard anterior"
                        .to_string()
                ),
            ]
        );
    }

    #[test]
    fn toggle_durante_processamento_inicia_nova_gravacao_sem_perder_a_anterior() {
        let (chegou_tx, chegou_rx) = mpsc::channel();
        let (liberar_tx, liberar_rx) = mpsc::channel();
        let engine = EngineBloqueante {
            chegou_tx,
            liberar_rx,
            texto: "primeiro ditado".to_string(),
        };
        let entrega = FakeEntrega::default();
        let feedback = FakeFeedback::default();
        let mut machine = Machine::new(
            feedback.clone(),
            FakeFonteDeAudio::default(),
            engine,
            entrega.clone(),
            FakeFoco::default(),
        );

        machine.toggle().unwrap(); // Ocioso -> Gravando
        machine.toggle().unwrap(); // Gravando -> Ocioso, despacha o Processando
        chegou_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("o Processando deveria ter chamado o Engine");

        // O Processando anterior ainda está bloqueado no Engine; o núcleo
        // já está livre para um novo Toggle.
        let resultado = machine.toggle().unwrap();
        assert_eq!(resultado, ResultadoToggle::Gravando);
        assert!(entrega.eventos().is_empty());

        liberar_tx.send(()).unwrap();
        machine.aguardar_processamentos();

        assert_eq!(
            entrega.eventos(),
            vec![
                EntregaEvento::Salvou,
                EntregaEvento::Copiou("primeiro ditado".to_string()),
                EntregaEvento::Colou(Atalho::Padrao),
                EntregaEvento::Restaurou(String::new()),
            ]
        );
        assert_eq!(
            feedback.events(),
            vec![
                Event::Iniciou,
                Event::Encerrou,
                Event::Iniciou,
                Event::Concluiu("primeiro ditado".to_string()),
            ]
        );
    }
}
