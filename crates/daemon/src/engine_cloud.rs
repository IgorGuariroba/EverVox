//! Engine cloud: transcreve a Gravação via API de transcrição de áudio da
//! OpenAI. Ao contrário do Engine local, cada Ditado depende de rede — falha
//! de rede ou da API vira [`ErroEngine`] e não cai para o Engine local (a
//! escolha do Engine é estática por config, ver `CONTEXT.md`).

use evervox_core::{AudioGravado, EngineSTT, ErroEngine};

const URL_TRANSCRICOES_OPENAI: &str = "https://api.openai.com/v1/audio/transcriptions";
const MODELO: &str = "whisper-1";

/// Nome do provedor sob o qual a chave de API é salva no GNOME Keyring (ver
/// `evervox_segredo` e `evervox set-key`).
pub const PROVEDOR_OPENAI: &str = "openai";

pub struct EngineCloud {
    client: reqwest::blocking::Client,
    url_transcricoes: String,
    chave_api: String,
    idioma: String,
}

impl EngineCloud {
    /// Constrói o Engine cloud contra a API da OpenAI. `chave_api` vem do
    /// GNOME Keyring (ver `evervox_segredo`), nunca de config ou ambiente.
    pub fn nova(chave_api: String, idioma: &str) -> Self {
        Self::com_url(URL_TRANSCRICOES_OPENAI.to_string(), chave_api, idioma)
    }

    fn com_url(url_transcricoes: String, chave_api: String, idioma: &str) -> Self {
        Self {
            client: reqwest::blocking::Client::new(),
            url_transcricoes,
            chave_api,
            idioma: idioma.to_string(),
        }
    }
}

#[derive(serde::Deserialize)]
struct RespostaTranscricao {
    text: String,
}

impl EngineSTT for EngineCloud {
    fn transcrever(&mut self, audio: &AudioGravado) -> Result<String, ErroEngine> {
        let wav = crate::wav::para_bytes(audio)
            .map_err(|erro| ErroEngine(format!("falha ao codificar o áudio: {erro}")))?;

        let parte = reqwest::blocking::multipart::Part::bytes(wav)
            .file_name("ditado.wav")
            .mime_str("audio/wav")
            .map_err(|erro| ErroEngine(format!("falha ao montar a requisição: {erro}")))?;

        let form = reqwest::blocking::multipart::Form::new()
            .part("file", parte)
            .text("model", MODELO)
            .text("language", self.idioma.clone());

        let resposta = self
            .client
            .post(&self.url_transcricoes)
            .bearer_auth(&self.chave_api)
            .multipart(form)
            .send()
            .map_err(|erro| {
                ErroEngine(format!("falha de rede ao chamar a API da OpenAI: {erro}"))
            })?;

        if !resposta.status().is_success() {
            let status = resposta.status();
            let corpo = resposta.text().unwrap_or_default();
            return Err(ErroEngine(format!(
                "API da OpenAI recusou a transcrição ({status}): {corpo}"
            )));
        }

        let corpo: RespostaTranscricao = resposta
            .json()
            .map_err(|erro| ErroEngine(format!("resposta inesperada da API da OpenAI: {erro}")))?;
        Ok(corpo.text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evervox_core::TAXA_AMOSTRAGEM_HZ;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// Sobe um servidor HTTP mínimo em `127.0.0.1` que aceita uma única
    /// conexão, ignora a requisição e responde com `resposta_http` (a
    /// resposta HTTP crua, incluindo status line e headers). Usado para
    /// testar o Engine cloud na costura com a API, sem rede real.
    fn servidor_mock(resposta_http: String) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endereco = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buffer = [0u8; 8192];
                let _ = stream.read(&mut buffer);
                let _ = stream.write_all(resposta_http.as_bytes());
                let _ = stream.flush();
            }
        });
        format!("http://{endereco}")
    }

    fn resposta_http(status: &str, corpo: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{corpo}",
            corpo.len()
        )
    }

    fn audio_de_teste() -> AudioGravado {
        AudioGravado {
            amostras: vec![1, 2, 3, 4, 5],
            taxa_amostragem_hz: TAXA_AMOSTRAGEM_HZ,
        }
    }

    #[test]
    fn fluxo_feliz_devolve_o_texto_transcrito() {
        let url = servidor_mock(resposta_http("200 OK", r#"{"text":"oi mundo"}"#));
        let mut engine = EngineCloud::com_url(url, "chave-de-teste".to_string(), "pt");

        let texto = engine.transcrever(&audio_de_teste()).unwrap();

        assert_eq!(texto, "oi mundo");
    }

    #[test]
    fn falha_da_api_vira_erro_claro_sem_expor_a_chave() {
        let url = servidor_mock(resposta_http(
            "401 Unauthorized",
            r#"{"error":"invalid_api_key"}"#,
        ));
        let mut engine = EngineCloud::com_url(url, "chave-secreta".to_string(), "pt");

        let erro = engine.transcrever(&audio_de_teste()).unwrap_err();

        assert!(erro.0.contains("401"));
        assert!(erro.0.contains("invalid_api_key"));
        assert!(!erro.0.contains("chave-secreta"));
    }
}
