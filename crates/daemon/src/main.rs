use evervox_core::{dbus, Feedback, Machine};
use notify_rust::Notification;
use std::process::Command;
use tokio::sync::Mutex;
use zbus::{connection, interface};

/// Feedback sonoro real: eventos do freedesktop sound theme via canberra.
struct SoundFeedback;

impl SoundFeedback {
    fn play(&self, event_id: &str) {
        if let Err(erro) = Command::new("canberra-gtk-play")
            .arg("-i")
            .arg(event_id)
            .spawn()
        {
            eprintln!("evervox-daemon: falha ao tocar som '{event_id}': {erro}");
        }
    }
}

impl Feedback for SoundFeedback {
    fn iniciou_gravacao(&mut self) {
        self.play("message-new-instant");
    }

    fn encerrou_gravacao(&mut self) {
        self.play("complete");
    }
}

struct DaemonService {
    machine: Mutex<Machine<SoundFeedback>>,
}

#[interface(name = "com.evervox.Daemon1")]
impl DaemonService {
    /// Aciona o Toggle do Ditado. Retorna o novo estado: "ocioso" | "gravando".
    async fn toggle(&self) -> String {
        let mut machine = self.machine.lock().await;
        machine.toggle().as_str().to_string()
    }
}

/// Verifica se o tocador de som do freedesktop sound theme está no PATH.
/// Sem ele o Daemon segue funcionando, só sem o beep de feedback do Toggle.
fn canberra_disponivel() -> bool {
    Command::new("which")
        .arg("canberra-gtk-play")
        .output()
        .map(|saida| saida.status.success())
        .unwrap_or(false)
}

async fn avisar_beep_indisponivel() {
    eprintln!(
        "evervox-daemon: 'canberra-gtk-play' não encontrado no PATH — \
         o Ditado seguirá sem beep sonoro."
    );
    let _ = Notification::new()
        .summary("EverVox")
        .body(
            "Som de feedback indisponível: instale o pacote com 'canberra-gtk-play' \
             (libcanberra) para ouvir o beep do Toggle.",
        )
        .show_async()
        .await;
}

#[tokio::main]
async fn main() -> zbus::Result<()> {
    if !canberra_disponivel() {
        avisar_beep_indisponivel().await;
    }

    let service = DaemonService {
        machine: Mutex::new(Machine::new(SoundFeedback)),
    };

    let connection = connection::Builder::session()?
        .serve_at(dbus::OBJECT_PATH, service)?
        .build()
        .await?;

    connection
        .request_name(dbus::SERVICE_NAME)
        .await
        .map_err(|erro| {
            eprintln!(
                "evervox-daemon: não foi possível registrar '{}' no D-Bus \
                 (já há um daemon rodando?): {erro}",
                dbus::SERVICE_NAME
            );
            erro
        })?;

    eprintln!(
        "evervox-daemon: pronto em {} ({}).",
        dbus::OBJECT_PATH,
        dbus::INTERFACE_NAME
    );
    std::future::pending::<()>().await;
    Ok(())
}
