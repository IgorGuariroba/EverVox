//! Limpeza por LLM: pós-processa a Transcrição crua removendo hesitações e
//! corrigindo gramática/pontuação, orientada pelas Instruções da Limpeza e
//! pelo Vocabulário do usuário (ver `CONTEXT.md`). Cada provedor (OpenAI,
//! Anthropic) implementa [`Limpeza`]; a escolha de provedor é estática por
//! config, como o Engine (ver [`crate::engine_cloud`]).
//!
//! Nota de design (ADR 0003): a costura aqui — um prompt de sistema montado a
//! partir do [`ContextoLimpeza`] e uma única chamada de LLM — nasce preparada
//! para fundir Limpeza + Tradução numa única chamada quando a Tradução for
//! implementada.

use evervox_core::{ErroLimpeza, Limpeza};
use std::time::Duration;

/// Nomes dos provedores sob os quais as chaves de API são salvas no GNOME
/// Keyring (ver [`evervox_segredo`] e `evervox set-key`). O provedor
/// `openai` é compartilhado com o Engine cloud (ver
/// [`crate::engine_cloud::PROVEDOR_OPENAI`]) — é a mesma chave de conta.
pub const PROVEDOR_OPENAI: &str = crate::engine_cloud::PROVEDOR_OPENAI;
pub const PROVEDOR_ANTHROPIC: &str = "anthropic";

const URL_OPENAI: &str = "https://api.openai.com/v1/chat/completions";
const URL_ANTHROPIC: &str = "https://api.anthropic.com/v1/messages";
const VERSAO_ANTHROPIC: &str = "2023-06-01";
const MAX_TOKENS_ANTHROPIC: u32 = 2_048;

/// Parâmetros do usuário que orientam a Limpeza (ver `CONTEXT.md`): texto
/// livre (Instruções da Limpeza), grafia de termos (Vocabulário) e se a
/// Pontuação falada deve virar os caracteres correspondentes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContextoLimpeza {
    pub instrucoes: String,
    pub vocabulario: Vec<String>,
    pub pontuacao_falada: bool,
}

/// Monta o prompt de sistema a partir do [`ContextoLimpeza`]: restrito a
/// limpar (nunca parafrasear, resumir ou inventar conteúdo), ver critério de
/// aceite da Limpeza.
fn prompt_sistema(contexto: &ContextoLimpeza) -> String {
    let mut prompt = String::from(
        "Você limpa transcrições de fala em texto: remove hesitações (\"éé\", \"tipo\", \
         \"né\"), corrige gramática e pontuação. Nunca parafraseia, resume, traduz ou \
         inventa conteúdo que não esteja no texto original — apenas limpa, preservando o \
         significado e as palavras do usuário. Responda somente com o texto limpo, sem \
         comentários, aspas ou explicações adicionais.",
    );

    if !contexto.instrucoes.trim().is_empty() {
        prompt.push_str("\n\nInstruções adicionais do usuário: ");
        prompt.push_str(contexto.instrucoes.trim());
    }

    if !contexto.vocabulario.is_empty() {
        prompt.push_str(
            "\n\nVocabulário do usuário (use esta grafia quando esses termos aparecerem): ",
        );
        prompt.push_str(&contexto.vocabulario.join(", "));
    }

    if contexto.pontuacao_falada {
        prompt.push_str(
            "\n\nConverta pontuação falada em caracteres: \"vírgula\" -> \",\", \"ponto\" -> \
             \".\", \"ponto de interrogação\" -> \"?\", \"ponto de exclamação\" -> \"!\", \
             \"dois pontos\" -> \":\", \"nova linha\" -> quebra de linha.",
        );
    } else {
        prompt.push_str(
            "\n\nNão converta palavras de pontuação faladas (\"vírgula\", \"ponto\" etc.) em \
             caracteres; mantenha-as como texto.",
        );
    }

    prompt
}

/// Constrói o cliente HTTP bloqueante compartilhado pelos provedores da
/// Limpeza, com o timeout do caminho crítico já embutido — defesa em
/// profundidade além do timeout que o núcleo já impõe (ver
/// `evervox_core::LimpezaExecucao`).
fn cliente_http(timeout: Duration) -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .timeout(timeout)
        .build()
        .expect("cliente HTTP da Limpeza deveria ser construível")
}

/// Envia a requisição e traduz falha de rede ou status HTTP de erro num
/// [`ErroLimpeza`] claro, sem expor a chave de API (que nunca entra no corpo
/// ou nos headers de erro devolvidos pelo provedor). Compartilhado pelos
/// provedores: só o corpo da requisição e o formato da resposta variam.
fn enviar_e_checar(
    requisicao: reqwest::blocking::RequestBuilder,
    servico: &str,
) -> Result<reqwest::blocking::Response, ErroLimpeza> {
    let resposta = requisicao.send().map_err(|erro| {
        ErroLimpeza(format!(
            "falha de rede ao chamar a API da {servico}: {erro}"
        ))
    })?;

    if !resposta.status().is_success() {
        let status = resposta.status();
        let corpo_erro = resposta.text().unwrap_or_default();
        return Err(ErroLimpeza(format!(
            "API da {servico} recusou a limpeza ({status}): {corpo_erro}"
        )));
    }

    Ok(resposta)
}

/// Usada quando `limpeza.habilitada = false` na config: o núcleo pula a
/// Limpeza inteiramente nesse caso (ver [`evervox_core::LimpezaExecucao`] e
/// `Machine`), então isto nunca é chamada de fato — mas o Daemon ainda
/// precisa de um `Box<dyn Limpeza>` concreto para montar a `Machine`, sem
/// exigir chave de API quando a Limpeza está desligada.
pub struct LimpezaDesativada;

impl Limpeza for LimpezaDesativada {
    fn limpar(&mut self, texto: &str) -> Result<String, ErroLimpeza> {
        Ok(texto.to_string())
    }
}

pub struct LimpezaOpenAI {
    client: reqwest::blocking::Client,
    url: String,
    chave_api: String,
    modelo: String,
    prompt_sistema: String,
}

impl LimpezaOpenAI {
    pub fn nova(
        chave_api: String,
        modelo: &str,
        contexto: &ContextoLimpeza,
        timeout: Duration,
    ) -> Self {
        Self::com_url(URL_OPENAI.to_string(), chave_api, modelo, contexto, timeout)
    }

    fn com_url(
        url: String,
        chave_api: String,
        modelo: &str,
        contexto: &ContextoLimpeza,
        timeout: Duration,
    ) -> Self {
        Self {
            client: cliente_http(timeout),
            url,
            chave_api,
            modelo: modelo.to_string(),
            prompt_sistema: prompt_sistema(contexto),
        }
    }
}

#[derive(serde::Serialize)]
struct MensagemOpenAI<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(serde::Serialize)]
struct RequisicaoOpenAI<'a> {
    model: &'a str,
    messages: Vec<MensagemOpenAI<'a>>,
    temperature: f32,
}

#[derive(serde::Deserialize)]
struct RespostaOpenAI {
    choices: Vec<EscolhaOpenAI>,
}

#[derive(serde::Deserialize)]
struct EscolhaOpenAI {
    message: MensagemRespostaOpenAI,
}

#[derive(serde::Deserialize)]
struct MensagemRespostaOpenAI {
    content: String,
}

impl Limpeza for LimpezaOpenAI {
    fn limpar(&mut self, texto: &str) -> Result<String, ErroLimpeza> {
        let corpo = RequisicaoOpenAI {
            model: &self.modelo,
            messages: vec![
                MensagemOpenAI {
                    role: "system",
                    content: &self.prompt_sistema,
                },
                MensagemOpenAI {
                    role: "user",
                    content: texto,
                },
            ],
            temperature: 0.0,
        };

        let resposta = enviar_e_checar(
            self.client
                .post(&self.url)
                .bearer_auth(&self.chave_api)
                .json(&corpo),
            "OpenAI",
        )?;

        let corpo: RespostaOpenAI = resposta
            .json()
            .map_err(|erro| ErroLimpeza(format!("resposta inesperada da API da OpenAI: {erro}")))?;
        corpo
            .choices
            .into_iter()
            .next()
            .map(|escolha| escolha.message.content)
            .ok_or_else(|| ErroLimpeza("resposta da API da OpenAI sem conteúdo".to_string()))
    }
}

pub struct LimpezaAnthropic {
    client: reqwest::blocking::Client,
    url: String,
    chave_api: String,
    modelo: String,
    prompt_sistema: String,
}

impl LimpezaAnthropic {
    pub fn nova(
        chave_api: String,
        modelo: &str,
        contexto: &ContextoLimpeza,
        timeout: Duration,
    ) -> Self {
        Self::com_url(
            URL_ANTHROPIC.to_string(),
            chave_api,
            modelo,
            contexto,
            timeout,
        )
    }

    fn com_url(
        url: String,
        chave_api: String,
        modelo: &str,
        contexto: &ContextoLimpeza,
        timeout: Duration,
    ) -> Self {
        Self {
            client: cliente_http(timeout),
            url,
            chave_api,
            modelo: modelo.to_string(),
            prompt_sistema: prompt_sistema(contexto),
        }
    }
}

#[derive(serde::Serialize)]
struct MensagemAnthropic<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(serde::Serialize)]
struct RequisicaoAnthropic<'a> {
    model: &'a str,
    max_tokens: u32,
    system: &'a str,
    messages: Vec<MensagemAnthropic<'a>>,
}

#[derive(serde::Deserialize)]
struct RespostaAnthropic {
    content: Vec<BlocoConteudoAnthropic>,
}

#[derive(serde::Deserialize)]
struct BlocoConteudoAnthropic {
    text: String,
}

impl Limpeza for LimpezaAnthropic {
    fn limpar(&mut self, texto: &str) -> Result<String, ErroLimpeza> {
        let corpo = RequisicaoAnthropic {
            model: &self.modelo,
            max_tokens: MAX_TOKENS_ANTHROPIC,
            system: &self.prompt_sistema,
            messages: vec![MensagemAnthropic {
                role: "user",
                content: texto,
            }],
        };

        let resposta = enviar_e_checar(
            self.client
                .post(&self.url)
                .header("x-api-key", &self.chave_api)
                .header("anthropic-version", VERSAO_ANTHROPIC)
                .json(&corpo),
            "Anthropic",
        )?;

        let corpo: RespostaAnthropic = resposta.json().map_err(|erro| {
            ErroLimpeza(format!("resposta inesperada da API da Anthropic: {erro}"))
        })?;
        corpo
            .content
            .into_iter()
            .next()
            .map(|bloco| bloco.text)
            .ok_or_else(|| ErroLimpeza("resposta da API da Anthropic sem conteúdo".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// Sobe um servidor HTTP mínimo em `127.0.0.1` que aceita uma única
    /// conexão, ignora a requisição e responde com `resposta_http` (a
    /// resposta HTTP crua, incluindo status line e headers) — mesmo padrão de
    /// teste usado em `crate::engine_cloud`.
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

    /// Sobe um servidor que aceita a conexão e nunca responde, forçando o
    /// timeout do cliente HTTP — usado para testar o timeout configurável.
    fn servidor_mock_sem_resposta() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endereco = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            if let Ok((_stream, _)) = listener.accept() {
                std::thread::sleep(Duration::from_secs(30));
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

    fn contexto_padrao() -> ContextoLimpeza {
        ContextoLimpeza::default()
    }

    #[test]
    fn prompt_de_sistema_inclui_instrucoes_vocabulario_e_pontuacao_falada() {
        let contexto = ContextoLimpeza {
            instrucoes: "expanda siglas".to_string(),
            vocabulario: vec!["EverVox".to_string(), "GNOME".to_string()],
            pontuacao_falada: true,
        };

        let prompt = prompt_sistema(&contexto);

        assert!(prompt.contains("expanda siglas"));
        assert!(prompt.contains("EverVox, GNOME"));
        assert!(prompt.contains("vírgula"));
        assert!(prompt.contains("Nunca parafraseia"));
    }

    #[test]
    fn prompt_de_sistema_sem_pontuacao_falada_instrui_a_nao_converter() {
        let contexto = ContextoLimpeza {
            pontuacao_falada: false,
            ..ContextoLimpeza::default()
        };

        let prompt = prompt_sistema(&contexto);

        assert!(prompt.contains("Não converta palavras de pontuação faladas"));
    }

    #[test]
    fn limpeza_desativada_devolve_o_texto_intacto() {
        let texto = LimpezaDesativada.limpar("oi mundo").unwrap();
        assert_eq!(texto, "oi mundo");
    }

    #[test]
    fn openai_fluxo_feliz_devolve_o_texto_limpo() {
        let url = servidor_mock(resposta_http(
            "200 OK",
            r#"{"choices":[{"message":{"content":"Oi, mundo."}}]}"#,
        ));
        let mut limpeza = LimpezaOpenAI::com_url(
            url,
            "chave-de-teste".to_string(),
            "gpt-4o-mini",
            &contexto_padrao(),
            Duration::from_secs(5),
        );

        let texto = limpeza.limpar("éé oi mundo").unwrap();

        assert_eq!(texto, "Oi, mundo.");
    }

    #[test]
    fn openai_falha_da_api_vira_erro_claro_sem_expor_a_chave() {
        let url = servidor_mock(resposta_http(
            "401 Unauthorized",
            r#"{"error":"invalid_api_key"}"#,
        ));
        let mut limpeza = LimpezaOpenAI::com_url(
            url,
            "chave-secreta".to_string(),
            "gpt-4o-mini",
            &contexto_padrao(),
            Duration::from_secs(5),
        );

        let erro = limpeza.limpar("oi mundo").unwrap_err();

        assert!(erro.0.contains("401"));
        assert!(!erro.0.contains("chave-secreta"));
    }

    #[test]
    fn openai_timeout_vira_erro_claro() {
        let url = servidor_mock_sem_resposta();
        let mut limpeza = LimpezaOpenAI::com_url(
            url,
            "chave-de-teste".to_string(),
            "gpt-4o-mini",
            &contexto_padrao(),
            Duration::from_millis(50),
        );

        let erro = limpeza.limpar("oi mundo").unwrap_err();

        assert!(erro.0.contains("falha de rede"));
    }

    #[test]
    fn anthropic_fluxo_feliz_devolve_o_texto_limpo() {
        let url = servidor_mock(resposta_http(
            "200 OK",
            r#"{"content":[{"type":"text","text":"Oi, mundo."}]}"#,
        ));
        let mut limpeza = LimpezaAnthropic::com_url(
            url,
            "chave-de-teste".to_string(),
            "claude-3-5-haiku-latest",
            &contexto_padrao(),
            Duration::from_secs(5),
        );

        let texto = limpeza.limpar("éé oi mundo").unwrap();

        assert_eq!(texto, "Oi, mundo.");
    }

    #[test]
    fn anthropic_falha_da_api_vira_erro_claro_sem_expor_a_chave() {
        let url = servidor_mock(resposta_http(
            "401 Unauthorized",
            r#"{"error":"invalid_api_key"}"#,
        ));
        let mut limpeza = LimpezaAnthropic::com_url(
            url,
            "chave-secreta".to_string(),
            "claude-3-5-haiku-latest",
            &contexto_padrao(),
            Duration::from_secs(5),
        );

        let erro = limpeza.limpar("oi mundo").unwrap_err();

        assert!(erro.0.contains("401"));
        assert!(!erro.0.contains("chave-secreta"));
    }
}
