use std::error::Error;
use std::io;
use std::time::Duration;
use tokio::time::sleep;
use zaman_sessiond::contract::{
    ForegroundStatusTuple, MenuStatusTuple, StatusTuple, INTERFACE, PATH, SERVICE,
};
use zbus::{Connection, Proxy};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let connection = Connection::session().await?;
    let proxy = Proxy::new(&connection, SERVICE, PATH, INTERFACE).await?;

    match arguments.as_slice() {
        [command] if command == "status" => {
            print_status(&read_status(&proxy).await?);
            print_foreground_status(&read_foreground_status(&proxy).await?);
        }
        [command] if command == "version" => {
            let version: String = proxy.call("Version", &()).await?;
            println!("{version}");
        }
        [command] if command == "stop" => {
            let _: () = proxy.call("Stop", &()).await?;
            wait_until_finished(&proxy, false).await?;
        }
        [command] if command == "menu-status" => print_menu_status(&read_menu_status(&proxy).await?),
        [command] if command == "resume" => {
            let _: () = proxy.call("Resume", &()).await?;
            wait_until_menu_closed(&proxy).await?;
        }
        [menu, action] if menu == "menu" && action == "open" => {
            let _: () = proxy.call("OpenMenu", &()).await?;
            print_foreground_status(&read_foreground_status(&proxy).await?);
        }
        [menu, action] if menu == "menu" && action == "close" => {
            let _: () = proxy.call("CloseMenu", &()).await?;
            print_foreground_status(&read_foreground_status(&proxy).await?);
        }
        [menu, action] if menu == "menu" && action == "toggle" => {
            let _: () = proxy.call("ToggleMenu", &()).await?;
            print_foreground_status(&read_foreground_status(&proxy).await?);
        }
        [command] if command == "exit-game" => {
            let _: () = proxy.call("ExitGame", &()).await?;
            wait_until_finished(&proxy, false).await?;
        }
        [command, system_id, rom_path] if command == "launch" => {
            let _: () = proxy.call("Launch", &(system_id, rom_path)).await?;
            wait_until_finished(&proxy, true).await?;
        }
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "usage: zamanctl status | menu {open|close|toggle} | menu-status | version | resume | exit-game | stop | launch SYSTEM_ID /absolute/ROM",
            )
            .into())
        }
    }

    Ok(())
}

async fn read_status(proxy: &Proxy<'_>) -> Result<StatusTuple> {
    Ok(proxy.call("Status", &()).await?)
}

async fn read_menu_status(proxy: &Proxy<'_>) -> Result<MenuStatusTuple> {
    Ok(proxy.call("MenuStatus", &()).await?)
}

async fn read_foreground_status(proxy: &Proxy<'_>) -> Result<ForegroundStatusTuple> {
    Ok(proxy.call("ForegroundStatus", &()).await?)
}

async fn wait_until_menu_closed(proxy: &Proxy<'_>) -> Result<()> {
    loop {
        let status = read_menu_status(proxy).await?;
        if !status.0 {
            print_menu_status(&status);
            return Ok(());
        }
        sleep(Duration::from_millis(50)).await;
    }
}

async fn wait_until_finished(proxy: &Proxy<'_>, require_result: bool) -> Result<()> {
    let mut previous_state = String::new();

    loop {
        let status = read_status(proxy).await?;
        if status.0 != previous_state {
            print_status(&status);
            previous_state.clone_from(&status.0);
        }

        match status.0.as_str() {
            "Idle" if require_result && status.4.is_empty() => {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "sessiond returned to Idle without a session result",
                )
                .into())
            }
            "Idle" => return Ok(()),
            "Failed" => {
                let error = if status.5.is_empty() {
                    "game session failed".to_string()
                } else {
                    status.5
                };
                return Err(io::Error::new(io::ErrorKind::Other, error).into());
            }
            _ => sleep(Duration::from_millis(100)).await,
        }
    }
}

fn print_status(status: &StatusTuple) {
    println!("state={}", display_value(&status.0));
    println!("system={}", display_value(&status.1));
    println!("emulator={}", display_value(&status.2));
    println!("rom={}", display_value(&status.3));
    println!("result={}", display_value(&status.4));
    println!("error={}", display_value(&status.5));
}

fn print_menu_status(status: &MenuStatusTuple) {
    println!("menu={}", if status.0 { "open" } else { "closed" });
    println!("generation={}", status.1);
    println!("reason={}", display_value(&status.2));
}

fn print_foreground_status(status: &ForegroundStatusTuple) {
    println!("foreground={}", display_value(&status.0));
    println!("return_target={}", display_value(&status.1));
    println!("foreground_reason={}", display_value(&status.2));
}

fn display_value(value: &str) -> &str {
    if value.is_empty() {
        "-"
    } else {
        value
    }
}
