//! Captura real de áudio via `cpal`, implementando a porta [`FonteDeAudio`]
//! do núcleo.
//!
//! O `cpal::Stream` não é `Send`, então ele é aberto, mantido vivo e
//! encerrado inteiramente dentro de uma thread dedicada; comunicação com o
//! resto do Daemon acontece por canais.
//!
//! O indicador de microfone do GNOME Shell acende para qualquer cliente que
//! abra um stream de captura via PipeWire/PulseAudio; o backend ALSA do
//! `cpal` chega até lá pelo roteamento padrão do ALSA para o servidor de som
//! do sistema. Isso depende da configuração do sistema (fora do controle
//! deste código) e não tem como ser verificado automaticamente em testes.

use crate::audio::{converter_para_pipeline, EstadoResample};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat;
use evervox_core::{ErroMicrofone, FonteDeAudio};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};

#[derive(Default)]
pub struct MicrofoneCpal {
    parar: Option<mpsc::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl FonteDeAudio for MicrofoneCpal {
    fn iniciar(&mut self, on_amostras: Box<dyn FnMut(&[i16]) + Send>) -> Result<(), ErroMicrofone> {
        let (parar_tx, parar_rx) = mpsc::channel::<()>();
        let (pronto_tx, pronto_rx) = mpsc::channel::<Result<(), ErroMicrofone>>();

        let handle = thread::spawn(move || {
            let mut on_amostras = on_amostras;
            match abrir_stream(move |amostras| on_amostras(amostras)) {
                Ok(stream) => {
                    if pronto_tx.send(Ok(())).is_err() {
                        return;
                    }
                    let _ = parar_rx.recv();
                    drop(stream);
                }
                Err(erro) => {
                    let _ = pronto_tx.send(Err(erro));
                }
            }
        });

        match pronto_rx.recv() {
            Ok(Ok(())) => {
                self.parar = Some(parar_tx);
                self.thread = Some(handle);
                Ok(())
            }
            Ok(Err(erro)) => {
                let _ = handle.join();
                Err(erro)
            }
            Err(_) => {
                let _ = handle.join();
                Err(ErroMicrofone(
                    "a thread de captura encerrou inesperadamente".to_string(),
                ))
            }
        }
    }

    fn encerrar(&mut self) {
        if let Some(tx) = self.parar.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

/// Abre o dispositivo de entrada padrão e inicia o stream de captura.
/// O `Stream` retornado precisa ser mantido vivo pelo chamador; ao ser
/// descartado, a captura para e o dispositivo é liberado.
fn abrir_stream(
    mut on_amostras: impl FnMut(&[i16]) + Send + 'static,
) -> Result<cpal::Stream, ErroMicrofone> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| ErroMicrofone("nenhum dispositivo de entrada padrão encontrado".into()))?;
    let config = device.default_input_config().map_err(|erro| {
        ErroMicrofone(format!(
            "não foi possível ler a configuração do dispositivo: {erro}"
        ))
    })?;

    let canais = config.channels();
    let taxa_origem = config.sample_rate().0;
    let formato = config.sample_format();
    let stream_config: cpal::StreamConfig = config.into();

    let erro_stream =
        |erro: cpal::StreamError| eprintln!("evervox-daemon: erro no stream de captura: {erro}");

    let mut estado = EstadoResample::default();

    let stream = match formato {
        SampleFormat::F32 => device.build_input_stream(
            &stream_config,
            move |dados: &[f32], _: &_| {
                let amostras = converter_para_pipeline(dados, canais, taxa_origem, &mut estado);
                on_amostras(&amostras);
            },
            erro_stream,
            None,
        ),
        SampleFormat::I16 => device.build_input_stream(
            &stream_config,
            move |dados: &[i16], _: &_| {
                let dados_f32: Vec<f32> =
                    dados.iter().map(|&s| s as f32 / i16::MAX as f32).collect();
                let amostras =
                    converter_para_pipeline(&dados_f32, canais, taxa_origem, &mut estado);
                on_amostras(&amostras);
            },
            erro_stream,
            None,
        ),
        SampleFormat::U16 => device.build_input_stream(
            &stream_config,
            move |dados: &[u16], _: &_| {
                let dados_f32: Vec<f32> = dados
                    .iter()
                    .map(|&s| (s as f32 - u16::MAX as f32 / 2.0) / (u16::MAX as f32 / 2.0))
                    .collect();
                let amostras =
                    converter_para_pipeline(&dados_f32, canais, taxa_origem, &mut estado);
                on_amostras(&amostras);
            },
            erro_stream,
            None,
        ),
        outro => {
            return Err(ErroMicrofone(format!(
                "formato de amostra do microfone não suportado: {outro:?}"
            )));
        }
    }
    .map_err(|erro| {
        ErroMicrofone(format!(
            "não foi possível abrir o stream de captura: {erro}"
        ))
    })?;

    stream
        .play()
        .map_err(|erro| ErroMicrofone(format!("não foi possível iniciar a captura: {erro}")))?;

    Ok(stream)
}
