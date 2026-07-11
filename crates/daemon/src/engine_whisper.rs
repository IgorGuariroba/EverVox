//! Engine local: transcreve a Gravação com whisper.cpp via `whisper-rs`.
//! O modelo é carregado uma única vez (em [`EngineWhisper::carregar`]) e
//! mantido na memória do Daemon; cada Ditado cria apenas um estado de
//! inferência novo e barato sobre o mesmo modelo.

use evervox_core::{AudioGravado, EngineSTT, ErroEngine, TAXA_AMOSTRAGEM_HZ};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

/// Abaixo disso não há amostras suficientes para o whisper.cpp processar de
/// forma confiável (ex.: um Toggle duplo quase instantâneo); tratamos como
/// Ditado silencioso sem nem chamar o Engine.
const AMOSTRAS_MINIMAS: usize = TAXA_AMOSTRAGEM_HZ as usize / 10;

pub struct EngineWhisper {
    contexto: WhisperContext,
    idioma: String,
}

impl EngineWhisper {
    /// Carrega o modelo do arquivo em `caminho_modelo`. Bloqueante: só deve
    /// ser chamado uma vez, na inicialização do Daemon.
    pub fn carregar(caminho_modelo: &std::path::Path, idioma: &str) -> anyhow::Result<Self> {
        let contexto =
            WhisperContext::new_with_params(caminho_modelo, WhisperContextParameters::default())?;
        Ok(Self {
            contexto,
            idioma: idioma.to_string(),
        })
    }
}

impl EngineSTT for EngineWhisper {
    fn transcrever(&mut self, audio: &AudioGravado) -> Result<String, ErroEngine> {
        if audio.amostras.len() < AMOSTRAS_MINIMAS {
            return Ok(String::new());
        }
        let amostras = amostras_para_f32(&audio.amostras);

        let mut estado = self
            .contexto
            .create_state()
            .map_err(|erro| ErroEngine(format!("não foi possível criar o estado: {erro}")))?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some(&self.idioma));
        params.set_translate(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_print_special(false);
        params.set_single_segment(false);

        estado
            .full(params, &amostras)
            .map_err(|erro| ErroEngine(format!("whisper.cpp falhou: {erro}")))?;

        let mut texto = String::new();
        for segmento in estado.as_iter() {
            if let Ok(trecho) = segmento.to_str_lossy() {
                if !e_marcador_sem_fala(&trecho) {
                    texto.push_str(&trecho);
                }
            }
        }

        Ok(texto)
    }
}

/// Converte amostras `i16` (o formato do núcleo) para `f32` normalizado em
/// `[-1.0, 1.0]`, o formato exigido pelo whisper.cpp.
fn amostras_para_f32(amostras: &[i16]) -> Vec<f32> {
    amostras
        .iter()
        .map(|&amostra| amostra as f32 / i16::MAX as f32)
        .collect()
}

/// whisper.cpp emite marcadores como `[BLANK_AUDIO]` ou `(silêncio)` para
/// trechos sem fala em vez de string vazia; sem filtrar isso, o Ditado
/// silencioso (critério de aceite) nunca dispararia com o Engine real.
fn e_marcador_sem_fala(trecho: &str) -> bool {
    let trecho = trecho.trim();
    trecho.is_empty()
        || (trecho.starts_with('[') && trecho.ends_with(']'))
        || (trecho.starts_with('(') && trecho.ends_with(')'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconhece_marcadores_sem_fala() {
        assert!(e_marcador_sem_fala(""));
        assert!(e_marcador_sem_fala("   "));
        assert!(e_marcador_sem_fala("[BLANK_AUDIO]"));
        assert!(e_marcador_sem_fala("(silêncio)"));
        assert!(!e_marcador_sem_fala("oi mundo"));
    }
}
