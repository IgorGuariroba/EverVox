//! Garante que o modelo GGML do whisper.cpp exista em disco, baixando-o do
//! repositório oficial na primeira execução. O Daemon carrega o arquivo
//! resultante uma única vez e o mantém na memória enquanto roda.

use std::path::PathBuf;

const URL_BASE: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

/// Garante que o modelo `nome` (ex.: `base`, `small`) esteja em
/// `$XDG_DATA_HOME/evervox/modelos/ggml-{nome}.bin`, baixando-o se ausente.
/// Retorna o caminho do arquivo.
pub fn garantir(nome: &str) -> anyhow::Result<PathBuf> {
    let caminho = crate::xdg::resolver("XDG_DATA_HOME", ".local/share")?
        .join("evervox/modelos")
        .join(format!("ggml-{nome}.bin"));
    if caminho.exists() {
        return Ok(caminho);
    }

    if let Some(dir) = caminho.parent() {
        std::fs::create_dir_all(dir)?;
    }

    eprintln!("evervox-daemon: baixando modelo '{nome}' (primeira execução)...");
    let url = format!("{URL_BASE}/ggml-{nome}.bin");
    let resposta = reqwest::blocking::get(&url)?.error_for_status()?;
    let bytes = resposta.bytes()?;

    let caminho_parcial = caminho.with_extension("bin.parcial");
    std::fs::write(&caminho_parcial, &bytes)?;
    std::fs::rename(&caminho_parcial, &caminho)?;

    eprintln!(
        "evervox-daemon: modelo '{nome}' salvo em {}",
        caminho.display()
    );
    Ok(caminho)
}
