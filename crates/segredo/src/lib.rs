//! Armazenamento de chaves de API no GNOME Keyring (Secret Service), usado
//! pelo Engine cloud. A chave nunca é escrita em config, log ou variável de
//! ambiente — este é o único ponto de acesso ao segredo.

use keyring::Entry;

/// Serviço sob o qual as chaves do EverVox são registradas no Keyring; o
/// `provedor` (ex.: `"openai"`) distingue a chave entre múltiplos serviços.
const SERVICO: &str = "evervox";

fn entrada(provedor: &str) -> anyhow::Result<Entry> {
    Entry::new(SERVICO, provedor)
        .map_err(|erro| anyhow::anyhow!("não foi possível acessar o GNOME Keyring: {erro}"))
}

/// Salva a chave de API do `provedor` no GNOME Keyring, substituindo
/// qualquer chave anterior.
pub fn salvar(provedor: &str, chave: &str) -> anyhow::Result<()> {
    entrada(provedor)?
        .set_password(chave)
        .map_err(|erro| anyhow::anyhow!("não foi possível salvar a chave no Keyring: {erro}"))
}

/// Carrega a chave de API do `provedor` do GNOME Keyring. `Ok(None)` indica
/// que nenhuma chave foi salva ainda (não é um erro: o chamador decide como
/// orientar o usuário a rodar `evervox set-key`).
pub fn carregar(provedor: &str) -> anyhow::Result<Option<String>> {
    match entrada(provedor)?.get_password() {
        Ok(chave) => Ok(Some(chave)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(erro) => Err(anyhow::anyhow!(
            "não foi possível ler a chave do Keyring: {erro}"
        )),
    }
}
