//! Entrega completa do Ditado (ADR 0001): clipboard + colar simulado. Salva
//! o clipboard atual via `wl-paste`, copia a Transcrição via `wl-copy`,
//! simula `Ctrl+V` com um teclado virtual `uinput` e restaura o clipboard
//! salvo. A detecção de terminal (`Ctrl+Shift+V`) fica para o próximo
//! ticket: aqui o colar é sempre `Ctrl+V`.

use evervox_core::{Entrega, ErroEntrega};
use std::io::Write;
use std::process::{Command, Stdio};
use uinput::event::keyboard;

/// Retrato do clipboard salvo antes de uma Entrega, para restaurar depois.
/// Preserva texto e imagem quando possível; um clipboard vazio ou de um
/// tipo não suportado é tratado como vazio (restaurar limpa o clipboard).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipboardSalvo {
    Texto(String),
    Binario { mime: String, dados: Vec<u8> },
    Vazio,
}

pub struct EntregaClipboard {
    teclado: Option<uinput::Device>,
}

impl EntregaClipboard {
    /// Tenta abrir um teclado virtual via uinput para o colar simulado. Se
    /// as permissões de `/dev/uinput` estiverem ausentes, a Entrega ainda
    /// funciona: só o colar automático fica indisponível, e o texto do
    /// Ditado permanece no clipboard como fallback manual (ver
    /// [`Entrega::colar`] em [`EntregaClipboard`]). Retorna também uma
    /// mensagem para o Daemon notificar na inicialização quando o teclado
    /// virtual não pôde ser criado.
    pub fn nova() -> (Self, Option<String>) {
        match criar_teclado_virtual() {
            Ok(teclado) => (
                Self {
                    teclado: Some(teclado),
                },
                None,
            ),
            Err(erro) => (
                Self { teclado: None },
                Some(format!(
                    "colar automático indisponível ({erro}). Verifique as permissões de \
                     /dev/uinput (usuário no grupo 'input' + regra udev, ver CONTEXT.md); \
                     o Ditado seguirá deixando o texto no clipboard para colar manualmente."
                )),
            ),
        }
    }
}

fn criar_teclado_virtual() -> anyhow::Result<uinput::Device> {
    uinput::default()
        .map_err(|erro| anyhow::anyhow!("{erro}"))?
        .name("evervox-colar-simulado")
        .map_err(|erro| anyhow::anyhow!("{erro}"))?
        .event(uinput::event::Keyboard::All)
        .map_err(|erro| anyhow::anyhow!("{erro}"))?
        .create()
        .map_err(|erro| anyhow::anyhow!("{erro}"))
}

impl Entrega for EntregaClipboard {
    type ClipboardSalvo = ClipboardSalvo;

    fn salvar_clipboard(&mut self) -> Result<ClipboardSalvo, ErroEntrega> {
        Ok(ler_clipboard_atual())
    }

    fn copiar(&mut self, texto: &str) -> Result<(), ErroEntrega> {
        copiar_para_clipboard(texto.as_bytes(), None)
    }

    fn colar(&mut self) -> Result<(), ErroEntrega> {
        let teclado = self
            .teclado
            .as_mut()
            .ok_or_else(|| ErroEntrega("teclado virtual (uinput) indisponível".to_string()))?;
        simular_ctrl_v(teclado)
    }

    fn restaurar_clipboard(&mut self, salvo: ClipboardSalvo) -> Result<(), ErroEntrega> {
        match salvo {
            ClipboardSalvo::Texto(texto) => copiar_para_clipboard(texto.as_bytes(), None),
            ClipboardSalvo::Binario { mime, dados } => copiar_para_clipboard(&dados, Some(&mime)),
            ClipboardSalvo::Vazio => limpar_clipboard(),
        }
    }
}

/// Roda `wl-paste` com os argumentos dados e devolve o stdout bruto, ou
/// `None` se o comando falhar (ex.: clipboard vazio, `wl-paste` ausente).
fn capturar_wl_paste(args: &[&str]) -> Option<Vec<u8>> {
    let saida = Command::new("wl-paste").args(args).output().ok()?;
    saida.status.success().then_some(saida.stdout)
}

fn listar_tipos_do_clipboard() -> Vec<String> {
    capturar_wl_paste(&["--list-types"])
        .map(|bytes| {
            String::from_utf8_lossy(&bytes)
                .lines()
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Lê o clipboard atual para poder restaurá-lo depois. Falhas de leitura
/// (clipboard vazio, `wl-paste` ausente) resultam em [`ClipboardSalvo::Vazio`]
/// em vez de erro: não há nada de errado em restaurar "nada" depois.
///
/// Texto é lido como UTF-8 estrito (não `from_utf8_lossy`): um clipboard de
/// texto com bytes inválidos é preservado como binário em vez de corrompido
/// silenciosamente por `�`. Qualquer outro tipo anunciado por `wl-paste
/// --list-types` (imagem, texto rico, lista de arquivos etc.) é preservado
/// como bytes crus no seu próprio mime — a Entrega não precisa entender o
/// conteúdo para devolvê-lo depois.
fn ler_clipboard_atual() -> ClipboardSalvo {
    let tipos = listar_tipos_do_clipboard();

    if tipos.iter().any(|tipo| tipo.starts_with("text/")) {
        if let Some(bytes) = capturar_wl_paste(&["--no-newline"]) {
            return match String::from_utf8(bytes) {
                Ok(texto) => ClipboardSalvo::Texto(texto),
                Err(erro) => ClipboardSalvo::Binario {
                    mime: "text/plain".to_string(),
                    dados: erro.into_bytes(),
                },
            };
        }
    }

    if let Some(mime) = tipos.first() {
        if let Some(dados) = capturar_wl_paste(&["-t", mime]) {
            return ClipboardSalvo::Binario {
                mime: mime.clone(),
                dados,
            };
        }
    }

    ClipboardSalvo::Vazio
}

fn copiar_para_clipboard(dados: &[u8], mime: Option<&str>) -> Result<(), ErroEntrega> {
    let mut comando = Command::new("wl-copy");
    if let Some(mime) = mime {
        comando.arg("-t").arg(mime);
    }

    let mut processo = comando
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|erro| ErroEntrega(format!("não foi possível iniciar 'wl-copy': {erro}")))?;

    processo
        .stdin
        .take()
        .expect("stdin foi configurado como piped")
        .write_all(dados)
        .map_err(|erro| ErroEntrega(format!("falha ao escrever no clipboard: {erro}")))?;

    let status = processo
        .wait()
        .map_err(|erro| ErroEntrega(format!("falha ao esperar 'wl-copy': {erro}")))?;

    if !status.success() {
        return Err(ErroEntrega(format!(
            "'wl-copy' terminou com erro: {status}"
        )));
    }
    Ok(())
}

fn limpar_clipboard() -> Result<(), ErroEntrega> {
    let status = Command::new("wl-copy")
        .arg("--clear")
        .status()
        .map_err(|erro| ErroEntrega(format!("não foi possível limpar o clipboard: {erro}")))?;

    if !status.success() {
        return Err(ErroEntrega(format!(
            "'wl-copy --clear' terminou com erro: {status}"
        )));
    }
    Ok(())
}

/// Tempo dado ao app focado para processar o `Ctrl+V` sintético e ler a
/// Transcrição do clipboard antes do núcleo restaurá-lo — sem essa pausa, a
/// restauração pode vencer a corrida com um app que lê o clipboard de forma
/// assíncrona, entregando o conteúdo antigo em vez da Transcrição.
const PAUSA_APOS_COLAR: std::time::Duration = std::time::Duration::from_millis(150);

fn simular_ctrl_v(teclado: &mut uinput::Device) -> Result<(), ErroEntrega> {
    (|| -> uinput::Result<()> {
        teclado.press(&keyboard::Key::LeftControl)?;
        teclado.click(&keyboard::Key::V)?;
        teclado.release(&keyboard::Key::LeftControl)?;
        teclado.synchronize()
    })()
    .map_err(|erro| ErroEntrega(format!("falha ao simular Ctrl+V: {erro}")))?;

    std::thread::sleep(PAUSA_APOS_COLAR);
    Ok(())
}
