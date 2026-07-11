//! Config TOML do Daemon: modelo local e idioma do Engine. Criada com
//! defaults na primeira execução, em `$XDG_CONFIG_HOME/evervox/config.toml`
//! (ou `~/.config/evervox/config.toml`).

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub idioma: String,
    pub modelo_local: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            idioma: "pt".to_string(),
            modelo_local: "base".to_string(),
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
    fn defaults_sao_pt_e_modelo_base() {
        let config = Config::default();
        assert_eq!(config.idioma, "pt");
        assert_eq!(config.modelo_local, "base");
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
