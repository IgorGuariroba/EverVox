//! Persistência do áudio de um Ditado em disco, como WAV, para inspeção.

use evervox_core::AudioGravado;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Salva o áudio completo de uma Gravação como WAV mono no diretório de
/// dados do usuário (`$XDG_DATA_HOME/evervox/ditados`, ou
/// `~/.local/share/evervox/ditados`) e retorna o caminho do arquivo.
pub fn salvar(audio: &AudioGravado) -> anyhow::Result<PathBuf> {
    let dir = diretorio_ditados()?;
    std::fs::create_dir_all(&dir)?;

    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
    let caminho = dir.join(format!("ditado-{timestamp}.wav"));

    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: audio.taxa_amostragem_hz,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(&caminho, spec)?;
    for &amostra in &audio.amostras {
        writer.write_sample(amostra)?;
    }
    writer.finalize()?;

    Ok(caminho)
}

fn diretorio_ditados() -> anyhow::Result<PathBuf> {
    if let Ok(xdg_data_home) = std::env::var("XDG_DATA_HOME") {
        return Ok(PathBuf::from(xdg_data_home).join("evervox/ditados"));
    }
    let home = std::env::var("HOME")
        .map_err(|_| anyhow::anyhow!("HOME não definido: não sei onde salvar o Ditado"))?;
    Ok(PathBuf::from(home).join(".local/share/evervox/ditados"))
}
