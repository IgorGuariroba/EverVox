//! Config TOML do Daemon: modelo local, idioma do Engine e lista de
//! terminais conhecidos (usada pela Entrega para decidir entre `Ctrl+V` e
//! `Ctrl+Shift+V`, ver [`crate::foco`]). Criada com defaults na primeira
//! execução, em `$XDG_CONFIG_HOME/evervox/config.toml` (ou
//! `~/.config/evervox/config.toml`).

use serde::{Deserialize, Serialize};
use std::path::Path;

/// O Engine STT a usar no Ditado, escolhido de forma estática pela config
/// (ver `CONTEXT.md`): nunca alterna sozinho em tempo de execução.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    /// whisper.cpp rodando na máquina (ver [`crate::engine_whisper`]).
    #[default]
    Local,
    /// API da OpenAI; exige chave salva via `evervox set-key openai` (ver
    /// [`crate::engine_cloud`]).
    Cloud,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub idioma: String,
    pub modelo_local: String,
    pub engine: Engine,
    /// Identificadores de app (WM_CLASS, como devolvido pela extensão GNOME)
    /// tratados como terminal na Entrega. Comparação sem diferenciar
    /// maiúsculas/minúsculas (ver [`crate::foco::decidir_atalho`]).
    pub terminais_conhecidos: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            idioma: "pt".to_string(),
            modelo_local: "base".to_string(),
            engine: Engine::default(),
            terminais_conhecidos: [
                "gnome-terminal-server",
                "org.gnome.terminal",
                "org.gnome.console",
                "kgx",
                "alacritty",
                "kitty",
                "konsole",
                "xterm",
                "tilix",
                "wezterm",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
        }
    }
}

/// Carrega a config do disco; se o arquivo não existir, cria com os
/// defaults e retorna esses defaults.
pub fn carregar_ou_criar() -> anyhow::Result<Config> {
    let caminho = crate::xdg::resolver("XDG_CONFIG_HOME", ".config")?
        .join("evervox")
        .join("config.toml");
    carregar_ou_criar_em(&caminho)
}

/// Idem, mas recebendo o caminho do arquivo diretamente — permite testar a
/// lógica de leitura/criação sem depender de variáveis de ambiente globais.
fn carregar_ou_criar_em(caminho: &Path) -> anyhow::Result<Config> {
    if let Ok(conteudo) = std::fs::read_to_string(caminho) {
        return Ok(toml::from_str(&conteudo)?);
    }

    let config = Config::default();
    if let Some(dir) = caminho.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(caminho, toml::to_string_pretty(&config)?)?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_sao_pt_modelo_base_e_engine_local() {
        let config = Config::default();
        assert_eq!(config.idioma, "pt");
        assert_eq!(config.modelo_local, "base");
        assert_eq!(config.engine, Engine::Local);
        assert!(config
            .terminais_conhecidos
            .contains(&"gnome-terminal-server".to_string()));
    }

    #[test]
    fn engine_cloud_e_lido_da_config_toml() {
        let config: Config = toml::from_str("engine = \"cloud\"").unwrap();
        assert_eq!(config.engine, Engine::Cloud);
    }

    #[test]
    fn carrega_ou_cria_escreve_defaults_e_os_relê_de_volta() {
        let dir_temporario = std::env::temp_dir().join(format!(
            "evervox-config-teste-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let caminho = dir_temporario.join("config.toml");

        let criada = carregar_ou_criar_em(&caminho).unwrap();
        assert_eq!(criada, Config::default());

        let relida = carregar_ou_criar_em(&caminho).unwrap();
        assert_eq!(relida, criada);

        std::fs::remove_dir_all(&dir_temporario).ok();
    }
}
