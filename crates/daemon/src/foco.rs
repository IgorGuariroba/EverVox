//! Porta de Foco (ADR 0001): consulta a extensão GNOME Shell via D-Bus para
//! saber o app focado e decide o [`Atalho`] de colar comparando contra a
//! lista de terminais conhecidos da config. Extensão ausente, incompatível,
//! ou qualquer falha na consulta degradam para [`Atalho::Padrao`] sem erro —
//! o pior caso é o mesmo `Ctrl+V` de antes desta extensão existir.

use evervox_core::{dbus_extensao, Atalho, Foco};
use zbus::blocking::Connection;

pub struct FocoGnome {
    connection: Option<Connection>,
    terminais_conhecidos: Vec<String>,
}

impl FocoGnome {
    /// Abre a conexão D-Bus de sessão. Se não for possível abri-la, todas as
    /// consultas degradam para [`Atalho::Padrao`] (ver [`Foco::atalho_de_colar`]).
    pub fn nova(terminais_conhecidos: Vec<String>) -> Self {
        Self {
            connection: Connection::session().ok(),
            terminais_conhecidos,
        }
    }

    /// Substitui a lista de terminais conhecidos (ver
    /// `crate::main::aplicar_campos_quentes`): campo quente das Preferências,
    /// aplicado sem reiniciar o Daemon.
    pub fn atualizar_terminais(&mut self, terminais_conhecidos: Vec<String>) {
        self.terminais_conhecidos = terminais_conhecidos;
    }

    fn consultar_app_focado(&self) -> Option<String> {
        let connection = self.connection.as_ref()?;
        let reply = connection
            .call_method(
                Some(dbus_extensao::SERVICE_NAME),
                dbus_extensao::OBJECT_PATH,
                Some(dbus_extensao::INTERFACE_NAME),
                dbus_extensao::METODO_APP_FOCADO,
                &(),
            )
            .ok()?;
        reply.body().deserialize::<String>().ok()
    }
}

impl Foco for FocoGnome {
    fn atalho_de_colar(&mut self) -> Atalho {
        let app_focado = self.consultar_app_focado();
        decidir_atalho(app_focado.as_deref(), &self.terminais_conhecidos)
    }
}

/// Decide o [`Atalho`] a partir do identificador do app focado (WM_CLASS,
/// como devolvido pela extensão GNOME) e da lista de terminais conhecidos da
/// config. `None` (extensão indisponível ou consulta falhou) degrada para
/// [`Atalho::Padrao`], igual a um app que não está na lista. A comparação
/// ignora maiúsculas/minúsculas: GNOME Shell não é consistente entre
/// versões sobre a capitalização do WM_CLASS.
fn decidir_atalho(app_focado: Option<&str>, terminais_conhecidos: &[String]) -> Atalho {
    let Some(app) = app_focado else {
        return Atalho::Padrao;
    };
    let e_terminal = terminais_conhecidos
        .iter()
        .any(|terminal| terminal.eq_ignore_ascii_case(app));
    if e_terminal {
        Atalho::Terminal
    } else {
        Atalho::Padrao
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terminais() -> Vec<String> {
        vec!["gnome-terminal-server".to_string(), "kitty".to_string()]
    }

    #[test]
    fn app_na_lista_de_terminais_decide_atalho_de_terminal() {
        assert_eq!(
            decidir_atalho(Some("gnome-terminal-server"), &terminais()),
            Atalho::Terminal
        );
    }

    #[test]
    fn comparacao_ignora_maiusculas_e_minusculas() {
        assert_eq!(
            decidir_atalho(Some("Gnome-Terminal-Server"), &terminais()),
            Atalho::Terminal
        );
    }

    #[test]
    fn app_fora_da_lista_decide_atalho_padrao() {
        assert_eq!(
            decidir_atalho(Some("firefox"), &terminais()),
            Atalho::Padrao
        );
    }

    #[test]
    fn app_focado_indisponivel_degrada_para_atalho_padrao() {
        assert_eq!(decidir_atalho(None, &terminais()), Atalho::Padrao);
    }

    #[test]
    fn atualizar_terminais_substitui_a_lista_usada_na_decisao() {
        let mut foco = FocoGnome::nova(terminais());
        assert_eq!(
            decidir_atalho(Some("firefox"), &foco.terminais_conhecidos),
            Atalho::Padrao
        );

        foco.atualizar_terminais(vec!["firefox".to_string()]);

        assert_eq!(
            decidir_atalho(Some("firefox"), &foco.terminais_conhecidos),
            Atalho::Terminal
        );
        assert_eq!(
            decidir_atalho(Some("gnome-terminal-server"), &foco.terminais_conhecidos),
            Atalho::Padrao
        );
    }
}
