//! Núcleo do Ditado: a máquina de estados acionada pelo Toggle.
//!
//! Nesta fase o núcleo só conhece Ocioso <-> Gravando; os estados e portas
//! de Processando (Engine, Limpeza, Entrega) chegam nos próximos tickets.

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

pub struct Machine<F: Feedback> {
    state: DitadoState,
    feedback: F,
}

impl<F: Feedback> Machine<F> {
    pub fn new(feedback: F) -> Self {
        Self {
            state: DitadoState::Ocioso,
            feedback,
        }
    }

    pub fn state(&self) -> DitadoState {
        self.state
    }

    /// Aciona o Toggle: Ocioso -> Gravando ou Gravando -> Ocioso.
    pub fn toggle(&mut self) -> DitadoState {
        self.state = match self.state {
            DitadoState::Ocioso => {
                self.feedback.iniciou_gravacao();
                DitadoState::Gravando
            }
            DitadoState::Gravando => {
                self.feedback.encerrou_gravacao();
                DitadoState::Ocioso
            }
        };
        self.state
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

    #[test]
    fn comeca_ocioso() {
        let machine = Machine::new(FakeFeedback::default());
        assert_eq!(machine.state(), DitadoState::Ocioso);
    }

    #[test]
    fn primeiro_toggle_inicia_gravacao_e_notifica() {
        let mut machine = Machine::new(FakeFeedback::default());

        let state = machine.toggle();

        assert_eq!(state, DitadoState::Gravando);
        assert_eq!(machine.feedback.events, vec![Event::Iniciou]);
    }

    #[test]
    fn toggle_duplo_alterna_estados_e_volta_ao_ocioso() {
        let mut machine = Machine::new(FakeFeedback::default());

        let apos_primeiro = machine.toggle();
        let apos_segundo = machine.toggle();

        assert_eq!(apos_primeiro, DitadoState::Gravando);
        assert_eq!(apos_segundo, DitadoState::Ocioso);
        assert_eq!(
            machine.feedback.events,
            vec![Event::Iniciou, Event::Encerrou]
        );
    }

    #[test]
    fn tres_toggles_termina_gravando() {
        let mut machine = Machine::new(FakeFeedback::default());

        machine.toggle();
        machine.toggle();
        let terceiro = machine.toggle();

        assert_eq!(terceiro, DitadoState::Gravando);
        assert_eq!(
            machine.feedback.events,
            vec![Event::Iniciou, Event::Encerrou, Event::Iniciou]
        );
    }
}
