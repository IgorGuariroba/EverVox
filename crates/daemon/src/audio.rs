//! Conversão do áudio bruto do dispositivo (taxa e número de canais
//! variáveis, amostras `f32`) para o formato exigido pela Gravação: `i16`
//! mono em [`TAXA_AMOSTRAGEM_HZ`](evervox_core::TAXA_AMOSTRAGEM_HZ).

use evervox_core::TAXA_AMOSTRAGEM_HZ;

/// Posição fracionária do downsample, mantida entre chamadas sucessivas do
/// callback de áudio para que a taxa de saída seja contínua através dos
/// blocos entregues pelo dispositivo.
#[derive(Default)]
pub struct EstadoResample {
    posicao_fracionaria: f64,
}

/// Faz o downmix para mono (média dos canais) e o downsample por
/// nearest-neighbor de `entrada` (intercalada, `canais` canais, taxa
/// `taxa_origem_hz`) para `TAXA_AMOSTRAGEM_HZ` mono em `i16`.
pub fn converter_para_pipeline(
    entrada: &[f32],
    canais: u16,
    taxa_origem_hz: u32,
    estado: &mut EstadoResample,
) -> Vec<i16> {
    let canais = canais.max(1) as usize;
    let quadros: Vec<f32> = entrada
        .chunks(canais)
        .map(|quadro| quadro.iter().sum::<f32>() / quadro.len() as f32)
        .collect();

    if quadros.is_empty() {
        return Vec::new();
    }

    let razao = taxa_origem_hz as f64 / TAXA_AMOSTRAGEM_HZ as f64;
    let mut saida = Vec::new();
    let mut posicao = estado.posicao_fracionaria;
    while (posicao as usize) < quadros.len() {
        let amostra = quadros[posicao as usize];
        saida.push((amostra.clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
        posicao += razao;
    }
    estado.posicao_fracionaria = posicao - quadros.len() as f64;
    saida
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mesma_taxa_mono_repassa_amostras() {
        let mut estado = EstadoResample::default();
        let entrada = [0.0_f32, 0.5, -0.5, 1.0];

        let saida = converter_para_pipeline(&entrada, 1, TAXA_AMOSTRAGEM_HZ, &mut estado);

        assert_eq!(saida.len(), 4);
        assert_eq!(saida[0], 0);
        assert_eq!(saida[3], i16::MAX);
    }

    #[test]
    fn taxa_dobrada_produz_metade_das_amostras() {
        let mut estado = EstadoResample::default();
        let entrada = vec![0.1_f32; 8];

        let saida = converter_para_pipeline(&entrada, 1, TAXA_AMOSTRAGEM_HZ * 2, &mut estado);

        assert_eq!(saida.len(), 4);
    }

    #[test]
    fn estereo_faz_downmix_pela_media_dos_canais() {
        let mut estado = EstadoResample::default();
        // Um quadro estéreo: canal esquerdo em 1.0, direito em -1.0 -> média 0.0
        let entrada = [1.0_f32, -1.0];

        let saida = converter_para_pipeline(&entrada, 2, TAXA_AMOSTRAGEM_HZ, &mut estado);

        assert_eq!(saida, vec![0]);
    }

    #[test]
    fn posicao_fracionaria_e_continua_entre_blocos() {
        let mut estado = EstadoResample::default();
        let bloco1 = vec![0.2_f32; 3];
        let bloco2 = vec![0.2_f32; 3];

        let taxa_origem = TAXA_AMOSTRAGEM_HZ * 3 / 2; // razão 1.5
        let saida1 = converter_para_pipeline(&bloco1, 1, taxa_origem, &mut estado);
        let saida2 = converter_para_pipeline(&bloco2, 1, taxa_origem, &mut estado);

        // 6 amostras de entrada a razão 1.5 devem produzir 4 amostras no total,
        // não 3 + 3, e não dependem de onde o bloco foi cortado.
        assert_eq!(saida1.len() + saida2.len(), 4);
    }
}
