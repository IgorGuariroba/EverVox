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
    /// Hint de transcrição montado a partir do Vocabulário do usuário (ver
    /// `CONTEXT.md`): nomes próprios e jargão que orientam a API a acertar a
    /// grafia. Vazio quando não há Vocabulário configurado.
    prompt_vocabulario: String,
}

impl EngineCloud {
    /// Constrói o Engine cloud contra a API da OpenAI. `chave_api` vem do
    /// GNOME Keyring (ver `evervox_segredo`), nunca de config ou ambiente.
    /// `vocabulario` vira o hint de transcrição enviado à API (ver
    /// `CONTEXT.md`).
    pub fn nova(chave_api: String, idioma: &str, vocabulario: &[String]) -> Self {
        Self::com_url(
            URL_TRANSCRICOES_OPENAI.to_string(),
            chave_api,
            idioma,
            vocabulario,
        )
    }

    fn com_url(
        url_transcricoes: String,
        chave_api: String,
        idioma: &str,
        vocabulario: &[String],
    ) -> Self {
        Self {
            client: reqwest::blocking::Client::new(),
            url_transcricoes,
            chave_api,
            idioma: idioma.to_string(),
            prompt_vocabulario: vocabulario.join(", "),
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

        let mut form = reqwest::blocking::multipart::Form::new()
            .part("file", parte)
            .text("model", MODELO)
            .text("language", self.idioma.clone());
        if !self.prompt_vocabulario.is_empty() {
            form = form.text("prompt", self.prompt_vocabulario.clone());
        }

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

    /// Atualiza o Idioma de entrada e o hint de Vocabulário sem reconstruir o
    /// cliente HTTP (ver `crate::main::recarregar_config`): campo quente, ao
    /// contrário de trocar de Engine (que exige restart, ver `CONTEXT.md`).
    fn atualizar_hint(&mut self, idioma: &str, vocabulario: &[String]) {
        self.idioma = idioma.to_string();
        self.prompt_vocabulario = vocabulario.join(", ");
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
        servidor_mock_capturando(resposta_http).0
    }

    /// Como [`servidor_mock`], mas também devolve a requisição crua recebida
    /// (headers e corpo), para inspecionar o que o Engine enviou.
    fn servidor_mock_capturando(
        resposta_http: String,
    ) -> (String, std::sync::mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endereco = listener.local_addr().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let requisicao = ler_requisicao_completa(&mut stream);
                let _ = tx.send(requisicao);
                let _ = stream.write_all(resposta_http.as_bytes());
                let _ = stream.flush();
            }
        });
        (format!("http://{endereco}"), rx)
    }

    /// Lê a requisição HTTP inteira do stream: acumula leituras até ter os
    /// headers completos e, a partir do `Content-Length`, o corpo inteiro.
    /// Uma única leitura não basta — o corpo multipart pode chegar em vários
    /// segmentos TCP, e parar cedo é exatamente a corrida que deixava os
    /// testes de inspeção da requisição intermitentes.
    fn ler_requisicao_completa(stream: &mut std::net::TcpStream) -> String {
        let mut dados = Vec::new();
        let mut buffer = [0u8; 65536];
        loop {
            let lidos = match stream.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(lidos) => lidos,
            };
            dados.extend_from_slice(&buffer[..lidos]);

            let texto = String::from_utf8_lossy(&dados);
            let Some(fim_headers) = texto.find("\r\n\r\n") else {
                continue;
            };
            let content_length: usize = texto[..fim_headers]
                .lines()
                .find_map(|linha| {
                    let (nome, valor) = linha.split_once(':')?;
                    nome.eq_ignore_ascii_case("content-length")
                        .then(|| valor.trim().parse().ok())?
                })
                .unwrap_or(0);
            if dados.len() >= fim_headers + 4 + content_length {
                break;
            }
        }
        String::from_utf8_lossy(&dados).to_string()
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
        let mut engine = EngineCloud::com_url(url, "chave-de-teste".to_string(), "pt", &[]);

        let texto = engine.transcrever(&audio_de_teste()).unwrap();

        assert_eq!(texto, "oi mundo");
    }

    #[test]
    fn falha_da_api_vira_erro_claro_sem_expor_a_chave() {
        let url = servidor_mock(resposta_http(
            "401 Unauthorized",
            r#"{"error":"invalid_api_key"}"#,
        ));
        let mut engine = EngineCloud::com_url(url, "chave-secreta".to_string(), "pt", &[]);

        let erro = engine.transcrever(&audio_de_teste()).unwrap_err();

        assert!(erro.0.contains("401"));
        assert!(erro.0.contains("invalid_api_key"));
        assert!(!erro.0.contains("chave-secreta"));
    }

    #[test]
    fn vocabulario_vira_hint_de_transcricao_na_requisicao() {
        let (url, requisicoes) =
            servidor_mock_capturando(resposta_http("200 OK", r#"{"text":"oi mundo"}"#));
        let vocabulario = vec!["EverVox".to_string(), "GNOME".to_string()];
        let mut engine =
            EngineCloud::com_url(url, "chave-de-teste".to_string(), "pt", &vocabulario);

        engine.transcrever(&audio_de_teste()).unwrap();

        let requisicao = requisicoes
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        assert!(requisicao.contains("EverVox, GNOME"));
    }

    #[test]
    fn sem_vocabulario_a_requisicao_nao_inclui_o_campo_prompt() {
        let (url, requisicoes) =
            servidor_mock_capturando(resposta_http("200 OK", r#"{"text":"oi mundo"}"#));
        let mut engine = EngineCloud::com_url(url, "chave-de-teste".to_string(), "pt", &[]);

        engine.transcrever(&audio_de_teste()).unwrap();

        let requisicao = requisicoes
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        assert!(!requisicao.contains("name=\"prompt\""));
    }

    #[test]
    fn atualizar_hint_troca_idioma_e_vocabulario_sem_reconstruir_o_engine() {
        let (url, requisicoes) =
            servidor_mock_capturando(resposta_http("200 OK", r#"{"text":"oi mundo"}"#));
        let mut engine = EngineCloud::com_url(url, "chave-de-teste".to_string(), "pt", &[]);

        engine.atualizar_hint("en", &["EverVox".to_string()]);
        engine.transcrever(&audio_de_teste()).unwrap();

        let requisicao = requisicoes
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        assert!(requisicao.contains("name=\"language\"\r\n\r\nen"));
        assert!(requisicao.contains("EverVox"));
    }
}
