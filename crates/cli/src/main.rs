use evervox_core::dbus;
use notify_rust::Notification;
use zbus::Connection;

#[tokio::main]
async fn main() {
    let comando = std::env::args().nth(1);
    match comando.as_deref() {
        Some("toggle") => toggle().await,
        _ => {
            eprintln!("uso: evervox toggle");
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
