//! Entrega mínima do Ditado: copia a Transcrição para o clipboard via
//! `wl-copy`. O colar simulado (Ctrl+V / Ctrl+Shift+V) chega no próximo
//! ticket; por ora o texto fica no clipboard para o usuário colar.

use evervox_core::{Entrega, ErroEntrega};
use std::io::Write;
use std::process::{Command, Stdio};

#[derive(Default)]
pub struct ClipboardWlCopy;

impl Entrega for ClipboardWlCopy {
    fn entregar(&mut self, texto: &str) -> Result<(), ErroEntrega> {
        let mut processo = Command::new("wl-copy")
            .stdin(Stdio::piped())
            .spawn()
            .map_err(|erro| ErroEntrega(format!("não foi possível iniciar 'wl-copy': {erro}")))?;

        processo
            .stdin
            .take()
            .expect("stdin foi configurado como piped")
            .write_all(texto.as_bytes())
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
}
