//! Núcleo do Ditado: a máquina de estados acionada pelo Toggle.
//!
//! Nesta fase o núcleo conhece Ocioso <-> Gravando e a captura de áudio da
//! Gravação; os estados e portas de Processando (Engine, Limpeza, Entrega)
//! chegam nos próximos tickets.

use std::sync::{Arc, Mutex};

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

/// Porta de feedback sensorial do Ditado (som, notificação).
/// Implementações reais ficam no Daemon; testes usam um fake.
pub trait Feedback {
    fn iniciou_gravacao(&mut self);
    fn encerrou_gravacao(&mut self);
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
    Ocioso { audio: AudioGravado },
}

impl ResultadoToggle {
    pub fn estado(&self) -> DitadoState {
        match self {
            ResultadoToggle::Gravando => DitadoState::Gravando,
            ResultadoToggle::Ocioso { .. } => DitadoState::Ocioso,
        }
    }
}

pub struct Machine<F: Feedback, G: FonteDeAudio> {
    state: DitadoState,
    feedback: F,
    gravador: Gravador<G>,
}

impl<F: Feedback, G: FonteDeAudio> Machine<F, G> {
    pub fn new(feedback: F, fonte_de_audio: G) -> Self {
        Self {
            state: DitadoState::Ocioso,
            feedback,
            gravador: Gravador::new(fonte_de_audio),
        }
    }

    pub fn state(&self) -> DitadoState {
        self.state
    }

    /// Aciona o Toggle: Ocioso -> Gravando ou Gravando -> Ocioso.
    ///
    /// Se o microfone estiver indisponível ao iniciar, o estado permanece
    /// Ocioso e nenhum feedback de início é disparado.
    pub fn toggle(&mut self) -> Result<ResultadoToggle, ErroMicrofone> {
        match self.state {
            DitadoState::Ocioso => {
                self.gravador.iniciar()?;
                self.state = DitadoState::Gravando;
                self.feedback.iniciou_gravacao();
                Ok(ResultadoToggle::Gravando)
            }
            DitadoState::Gravando => {
                let audio = self.gravador.encerrar();
                self.state = DitadoState::Ocioso;
                self.feedback.encerrou_gravacao();
                Ok(ResultadoToggle::Ocioso { audio })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq, Eq)]
    enum Event {
        Iniciou,
        Encerrou,
    }

    #[derive(Default)]
    struct FakeFeedback {
        events: Vec<Event>,
    }

    impl Feedback for FakeFeedback {
        fn iniciou_gravacao(&mut self) {
            self.events.push(Event::Iniciou);
        }

        fn encerrou_gravacao(&mut self) {
            self.events.push(Event::Encerrou);
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

    fn nova_machine(fonte: FakeFonteDeAudio) -> Machine<FakeFeedback, FakeFonteDeAudio> {
        Machine::new(FakeFeedback::default(), fonte)
    }

    #[test]
    fn comeca_ocioso() {
        let machine = nova_machine(FakeFonteDeAudio::default());
        assert_eq!(machine.state(), DitadoState::Ocioso);
    }

    #[test]
    fn primeiro_toggle_inicia_gravacao_e_notifica() {
        let mut machine = nova_machine(FakeFonteDeAudio::default());

        let resultado = machine.toggle().unwrap();

        assert_eq!(resultado, ResultadoToggle::Gravando);
        assert_eq!(machine.feedback.events, vec![Event::Iniciou]);
    }

    #[test]
    fn toggle_duplo_alterna_estados_e_volta_ao_ocioso() {
        let mut machine = nova_machine(FakeFonteDeAudio::default());

        let apos_primeiro = machine.toggle().unwrap();
        let apos_segundo = machine.toggle().unwrap();

        assert_eq!(apos_primeiro.estado(), DitadoState::Gravando);
        assert_eq!(apos_segundo.estado(), DitadoState::Ocioso);
        assert_eq!(
            machine.feedback.events,
            vec![Event::Iniciou, Event::Encerrou]
        );
    }

    #[test]
    fn tres_toggles_termina_gravando() {
        let mut machine = nova_machine(FakeFonteDeAudio::default());

        machine.toggle().unwrap();
        machine.toggle().unwrap();
        let terceiro = machine.toggle().unwrap();

        assert_eq!(terceiro.estado(), DitadoState::Gravando);
        assert_eq!(
            machine.feedback.events,
            vec![Event::Iniciou, Event::Encerrou, Event::Iniciou]
        );
    }

    #[test]
    fn ao_encerrar_o_audio_completo_chega_a_proxima_etapa() {
        let fonte = FakeFonteDeAudio {
            blocos: vec![vec![1, 2, 3], vec![4, 5]],
            falha: None,
        };
        let mut machine = nova_machine(fonte);

        machine.toggle().unwrap(); // inicia a gravação, a fonte emite os blocos
        let resultado = machine.toggle().unwrap(); // encerra

        match resultado {
            ResultadoToggle::Ocioso { audio } => {
                assert_eq!(audio.amostras, vec![1, 2, 3, 4, 5]);
                assert_eq!(audio.taxa_amostragem_hz, TAXA_AMOSTRAGEM_HZ);
            }
            other => panic!("esperava Ocioso com áudio, obteve {other:?}"),
        }
    }

    #[test]
    fn falha_de_microfone_mantem_ocioso_e_nao_notifica() {
        let fonte = FakeFonteDeAudio {
            blocos: vec![],
            falha: Some("dispositivo indisponível".to_string()),
        };
        let mut machine = nova_machine(fonte);

        let erro = machine.toggle().unwrap_err();

        assert_eq!(erro, ErroMicrofone("dispositivo indisponível".to_string()));
        assert_eq!(machine.state(), DitadoState::Ocioso);
        assert!(machine.feedback.events.is_empty());
    }
}
