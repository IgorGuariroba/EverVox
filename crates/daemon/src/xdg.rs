//! Resolução de diretórios XDG usados pelo Daemon (config, dados, modelos):
//! `$<variavel>` se definida, senão `$HOME/<fallback>`.

use std::path::PathBuf;

pub fn resolver(variavel: &str, fallback_em_home: &str) -> anyhow::Result<PathBuf> {
    if let Ok(dir) = std::env::var(variavel) {
        return Ok(PathBuf::from(dir));
    }
    let home = std::env::var("HOME").map_err(|_| {
        anyhow::anyhow!("HOME não definido: não sei onde ler/gravar dados do EverVox")
    })?;
    Ok(PathBuf::from(home).join(fallback_em_home))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usa_a_variavel_quando_definida() {
        std::env::set_var("EVERVOX_TESTE_XDG", "/tmp/algum-lugar");
        let resolvido = resolver("EVERVOX_TESTE_XDG", ".config/evervox").unwrap();
        std::env::remove_var("EVERVOX_TESTE_XDG");
        assert_eq!(resolvido, PathBuf::from("/tmp/algum-lugar"));
    }
}
