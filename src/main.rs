mod command;
mod error;
mod inputplumber;
mod session;
mod systemd;

use crate::command::CommandSpec;
use crate::error::Result;
use crate::inputplumber::InputPlumber;
use crate::session::Session;
use crate::systemd::UserSystemd;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let command = CommandSpec::from_env()?;
    let input = InputPlumber::connect().await?;
    let inventory = input.discover().await?;
    let systemd = UserSystemd::connect().await?;

    println!(
        "Discovered {} composite device(s) and {} normalized D-Bus target(s).",
        inventory.composite_count(),
        inventory.target_count()
    );

    let mut session = Session::new(input, systemd, inventory);
    let outcome = session.run(&command).await?;

    println!("Session finished: {outcome}.");
    println!("Final session state: {}.", session.state());

    Ok(())
}
