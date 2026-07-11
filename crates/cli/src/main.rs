use evervox_core::{dbus, dbus_extensao};
use notify_rust::Notification;
use zbus::Connection;

/// Provedores de API cujas chaves `evervox status` confere no GNOME
/// Keyring, independente de qual a config do Daemon exige no momento.
const PROVEDORES_DE_CHAVE: &[&str] = &["openai", "anthropic"];

#[tokio::main]
async fn main() {
    let comando = std::env::args().nth(1);
    match comando.as_deref() {
        Some("toggle") => toggle().await,
        Some("set-key") => set_key(std::env::args().nth(2)).await,
        Some("status") => status().await,
        _ => {
            eprintln!("uso: evervox toggle | evervox set-key <provedor> | evervox status");
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

/// Reporta a saúde do EverVox (ver issue #10): Daemon ativo, extensão GNOME
/// respondendo e chaves de API salvas no Keyring. Cada checagem roda de
/// forma independente das outras, para o diagnóstico continuar útil mesmo
/// com parte do sistema fora do ar.
async fn status() {
    println!("EverVox — status\n");

    match consultar_status_daemon().await {
        Ok(resumo) => {
            println!("Daemon: ativo");
            for linha in resumo.lines() {
                println!("  {linha}");
            }
        }
        Err(erro) => {
            println!("Daemon: não está rodando ({erro})");
            println!("  dica: systemctl --user status evervox");
        }
    }

    match consultar_extensao().await {
        Ok(()) => println!("Extensão GNOME: respondendo"),
        Err(_) => {
            println!("Extensão GNOME: não detectada (Entrega usa Ctrl+V como fallback)")
        }
    }

    println!("Chaves de API:");
    for provedor in PROVEDORES_DE_CHAVE {
        let situacao = match evervox_segredo::carregar(provedor) {
            Ok(Some(_)) => "salva",
            Ok(None) => "não configurada",
            Err(erro) => {
                eprintln!("evervox: falha ao consultar a chave de '{provedor}' no Keyring: {erro}");
                "erro ao consultar o Keyring"
            }
        };
        println!("  {provedor}: {situacao}");
    }
}

/// Chama `Status()` no Daemon via D-Bus. Erro aqui cobre tanto "Daemon não
/// está rodando" quanto qualquer falha de D-Bus na consulta.
async fn consultar_status_daemon() -> anyhow::Result<String> {
    let connection = Connection::session().await?;
    let reply = connection
        .call_method(
            Some(dbus::SERVICE_NAME),
            dbus::OBJECT_PATH,
            Some(dbus::INTERFACE_NAME),
            "Status",
            &(),
        )
        .await?;
    Ok(reply.body().deserialize()?)
}

/// Chama `AppFocado()` na extensão GNOME via D-Bus, só para confirmar que
/// ela está instalada, habilitada e respondendo — o valor devolvido não
/// importa para `evervox status`.
async fn consultar_extensao() -> anyhow::Result<()> {
    let connection = Connection::session().await?;
    connection
        .call_method(
            Some(dbus_extensao::SERVICE_NAME),
            dbus_extensao::OBJECT_PATH,
            Some(dbus_extensao::INTERFACE_NAME),
            dbus_extensao::METODO_APP_FOCADO,
            &(),
        )
        .await?;
    Ok(())
}
