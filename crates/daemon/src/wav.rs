//! Codificação WAV do áudio de uma Gravação — em disco, para inspeção, ou em
//! memória, para envio ao Engine cloud (ver [`crate::engine_cloud`]).

use evervox_core::AudioGravado;
use std::io::Cursor;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Especificação WAV mono 16-bit usada em toda codificação do áudio de uma
/// Gravação: o formato produzido pelo núcleo (`i16`) já é o exigido aqui.
fn spec(audio: &AudioGravado) -> hound::WavSpec {
    hound::WavSpec {
        channels: 1,
        sample_rate: audio.taxa_amostragem_hz,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    }
}

/// Salva o áudio completo de uma Gravação como WAV mono no diretório de
/// dados do usuário (`$XDG_DATA_HOME/evervox/gravacoes`, ou
/// `~/.local/share/evervox/gravacoes`) e retorna o caminho do arquivo.
pub fn salvar(audio: &AudioGravado) -> anyhow::Result<PathBuf> {
    let dir = crate::xdg::resolver("XDG_DATA_HOME", ".local/share")?.join("evervox/gravacoes");
    std::fs::create_dir_all(&dir)?;

    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
    let caminho = dir.join(format!("gravacao-{timestamp}.wav"));

    let mut writer = hound::WavWriter::create(&caminho, spec(audio))?;
    for &amostra in &audio.amostras {
        writer.write_sample(amostra)?;
    }
    writer.finalize()?;

    Ok(caminho)
}

/// Codifica o áudio completo de uma Gravação como WAV mono em memória, sem
/// tocar o disco — usado pelo Engine cloud para enviar o áudio à API.
pub fn para_bytes(audio: &AudioGravado) -> anyhow::Result<Vec<u8>> {
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut writer = hound::WavWriter::new(&mut cursor, spec(audio))?;
        for &amostra in &audio.amostras {
            writer.write_sample(amostra)?;
        }
        writer.finalize()?;
    }
    Ok(cursor.into_inner())
}
