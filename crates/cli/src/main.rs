use evervox_core::dbus;
use notify_rust::Notification;
use zbus::Connection;

#[tokio::main]
async fn main() {
    let comando = std::env::args().nth(1);
    match comando.as_deref() {
        Some("toggle") => toggle().await,
        Some("set-key") => set_key(std::env::args().nth(2)).await,
        _ => {
            eprintln!("uso: evervox toggle | evervox set-key <provedor>");
            std::process::exit(1);
        }
    }
}

/// Lê a chave de API do provedor (ex.: `openai`) de forma oculta no
/// terminal e a salva no GNOME Keyring via `evervox_segredo`. A chave nunca
/// passa pelo Daemon, pela config ou por variável de ambiente.
async fn set_key(provedor: Option<String>) {
    let Some(provedor) = provedor else {
        eprintln!("uso: evervox set-key <provedor>");
        std::process::exit(1);
    };

    let chave = match rpassword::prompt_password(format!("Chave de API para '{provedor}': ")) {
        Ok(chave) => chave,
        Err(erro) => {
            eprintln!("evervox: não foi possível ler a chave: {erro}");
            std::process::exit(1);
        }
    };
    if chave.trim().is_empty() {
        eprintln!("evervox: chave vazia, nada foi salvo.");
        std::process::exit(1);
    }

    match evervox_segredo::salvar(&provedor, &chave) {
        Ok(()) => println!("Chave de '{provedor}' salva no GNOME Keyring."),
        Err(erro) => {
            eprintln!("evervox: falha ao salvar a chave: {erro}");
            std::process::exit(1);
        }
    }
}

async fn toggle() {
    match enviar_toggle().await {
        Ok(estado) => println!("{estado}"),
        Err(erro) => {
            eprintln!("evervox: {erro}");
            notificar_daemon_indisponivel(&erro.to_string()).await;
            std::process::exit(1);
        }
    }
}

async fn enviar_toggle() -> anyhow::Result<String> {
    let connection = Connection::session().await?;
    let reply = connection
        .call_method(
            Some(dbus::SERVICE_NAME),
            dbus::OBJECT_PATH,
            Some(dbus::INTERFACE_NAME),
            "Toggle",
            &(),
        )
        .await?;
    let estado: String = reply.body().deserialize()?;
    Ok(estado)
}

async fn notificar_daemon_indisponivel(detalhe: &str) {
    let _ = Notification::new()
        .summary("EverVox")
        .body(&format!("Daemon não está rodando: {detalhe}"))
        .show_async()
        .await;
}
