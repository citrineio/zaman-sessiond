use futures_util::StreamExt;
use std::collections::BTreeSet;
use std::error::Error;
use std::io;
use std::time::Duration;
use tokio::task::JoinSet;
use tokio::time::timeout;
use zbus::fdo::ObjectManagerProxy;
use zbus::zvariant::OwnedObjectPath;
use zbus::{Connection, Proxy};

const INPUTPLUMBER_SERVICE: &str = "org.shadowblip.InputPlumber";
const INPUTPLUMBER_ROOT: &str = "/org/shadowblip/InputPlumber";
const COMPOSITE_INTERFACE: &str = "org.shadowblip.Input.CompositeDevice";
const DBUS_DEVICE_INTERFACE: &str = "org.shadowblip.Input.DBusDevice";

const SYSTEMD_SERVICE: &str = "org.freedesktop.systemd1";
const SYSTEMD_PATH: &str = "/org/freedesktop/systemd1";
const SYSTEMD_MANAGER_INTERFACE: &str = "org.freedesktop.systemd1.Manager";

const INTERCEPT_NONE: u32 = 0;
const INTERCEPT_PASS: u32 = 1;

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

struct Composite {
    path: String,
    name: String,
}

struct Inventory {
    composites: Vec<Composite>,
    dbus_targets: BTreeSet<String>,
}

enum StopReason {
    Guide { target: String },
    CtrlC,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let validation_unit = std::env::var("ZAMAN_VALIDATION_UNIT").map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "ZAMAN_VALIDATION_UNIT must name the active test game unit",
        )
    })?;

    let system_connection = Connection::system().await?;
    let user_connection = Connection::session().await?;
    let inventory = discover_inputplumber(&system_connection).await?;

    println!(
        "Discovered {} composite device(s) and {} D-Bus target(s).",
        inventory.composites.len(),
        inventory.dbus_targets.len()
    );

    let session_result = run_validation_session(
        &system_connection,
        &user_connection,
        &inventory,
        &validation_unit,
    )
    .await;

    // Cleanup occurs regardless of Guide, Ctrl-C, or listener failure.
    let cleanup_result =
        set_intercept_mode(&system_connection, &inventory.composites, INTERCEPT_NONE).await;

    match (session_result, cleanup_result) {
        (Ok(StopReason::Guide { target }), Ok(())) => {
            println!("Guide exit request received from {target}");
            println!("InterceptMode restored to NONE.");
            Ok(())
        }
        (Ok(StopReason::CtrlC), Ok(())) => {
            println!("Validation interrupted with Ctrl-C.");
            println!("InterceptMode restored to NONE.");
            Ok(())
        }
        (Err(session_error), Ok(())) => Err(session_error),
        (Ok(_), Err(cleanup_error)) => Err(cleanup_error),
        (Err(session_error), Err(cleanup_error)) => Err(format!(
            "session failed: {session_error}; cleanup also failed: {cleanup_error}"
        )
        .into()),
    }
}

async fn run_validation_session(
    input_connection: &Connection,
    systemd_connection: &Connection,
    inventory: &Inventory,
    validation_unit: &str,
) -> Result<StopReason> {
    set_intercept_mode(input_connection, &inventory.composites, INTERCEPT_PASS).await?;

    println!("InterceptMode set to PASS.");
    println!("Press A, then Guide. Ctrl-C also performs cleanup.");

    let reason = wait_for_stop_request(input_connection, &inventory.dbus_targets).await?;

    if matches!(&reason, StopReason::Guide { .. }) {
        stop_user_unit(systemd_connection, validation_unit).await?;
    }

    Ok(reason)
}

async fn stop_user_unit(connection: &Connection, unit: &str) -> Result<()> {
    let manager = Proxy::new(
        connection,
        SYSTEMD_SERVICE,
        SYSTEMD_PATH,
        SYSTEMD_MANAGER_INTERFACE,
    )
    .await?;

    let _: () = manager.call("Subscribe", &()).await?;
    let mut removed_jobs = manager.receive_signal("JobRemoved").await?;

    let job_path: OwnedObjectPath = manager.call("StopUnit", &(unit, "replace")).await?;

    println!("Queued systemd stop job {job_path} for {unit}.");

    let wait_for_job = async {
        while let Some(message) = removed_jobs.next().await {
            let (_id, path, job_unit, result): (u32, OwnedObjectPath, String, String) =
                message.body().deserialize()?;

            if path != job_path {
                continue;
            }

            if job_unit != unit || result != "done" {
                return Err::<(), Box<dyn Error + Send + Sync>>(
                    format!("stop job failed: unit={job_unit}, result={result}").into(),
                );
            }

            println!("systemd confirmed {unit} stopped.");
            return Ok::<(), Box<dyn Error + Send + Sync>>(());
        }

        Err::<(), Box<dyn Error + Send + Sync>>("systemd JobRemoved stream closed".into())
    };

    timeout(Duration::from_secs(10), wait_for_job)
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "systemd stop timed out"))?
}

async fn discover_inputplumber(connection: &Connection) -> Result<Inventory> {
    let object_manager = ObjectManagerProxy::builder(connection)
        .destination(INPUTPLUMBER_SERVICE)?
        .path(INPUTPLUMBER_ROOT)?
        .build()
        .await?;

    let managed_objects = object_manager.get_managed_objects().await?;
    let mut composites = Vec::new();
    let mut dbus_targets = BTreeSet::new();

    for (path, interfaces) in managed_objects {
        let is_composite = interfaces
            .keys()
            .any(|interface| interface.as_str() == COMPOSITE_INTERFACE);

        if !is_composite {
            continue;
        }

        let proxy = Proxy::new(
            connection,
            INPUTPLUMBER_SERVICE,
            path.as_str(),
            COMPOSITE_INTERFACE,
        )
        .await?;

        let name: String = proxy.get_property("Name").await?;
        let persistent_id: String = proxy.get_property("PersistentId").await?;
        let targets: Vec<String> = proxy.get_property("DbusDevices").await?;

        println!("Composite: {name}");
        println!("  runtime path: {path}");
        println!("  persistent ID: {persistent_id}");

        for target in targets {
            println!("  runtime D-Bus target: {target}");
            dbus_targets.insert(target);
        }

        composites.push(Composite {
            path: path.to_string(),
            name,
        });
    }

    if composites.is_empty() {
        return Err("InputPlumber reported no composite devices".into());
    }

    if dbus_targets.is_empty() {
        return Err("No InputPlumber D-Bus targets were discovered".into());
    }

    Ok(Inventory {
        composites,
        dbus_targets,
    })
}

async fn set_intercept_mode(
    connection: &Connection,
    composites: &[Composite],
    mode: u32,
) -> Result<()> {
    for composite in composites {
        let proxy = Proxy::new(
            connection,
            INPUTPLUMBER_SERVICE,
            composite.path.as_str(),
            COMPOSITE_INTERFACE,
        )
        .await?;

        proxy.set_property("InterceptMode", mode).await?;

        println!("Set {} InterceptMode to {}.", composite.name, mode);
    }

    Ok(())
}

async fn wait_for_stop_request(
    connection: &Connection,
    targets: &BTreeSet<String>,
) -> Result<StopReason> {
    let mut listeners = JoinSet::new();

    for target in targets {
        let connection = connection.clone();
        let target = target.clone();

        listeners.spawn(async move { wait_for_guide(connection, target).await });
    }

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                listeners.abort_all();
                return Ok(StopReason::CtrlC);
            }

            completed = listeners.join_next() => {
                match completed {
                    Some(Ok(Ok(target))) => {
                        listeners.abort_all();
                        return Ok(StopReason::Guide { target });
                    }
                    Some(Ok(Err(error))) => {
                        eprintln!("Input listener failed: {error}");
                    }
                    Some(Err(error)) => {
                        eprintln!("Input listener task failed: {error}");
                    }
                    None => {
                        return Err("All input listeners stopped".into());
                    }
                }
            }
        }
    }
}

async fn wait_for_guide(connection: Connection, target: String) -> Result<String> {
    let proxy = Proxy::new(
        &connection,
        INPUTPLUMBER_SERVICE,
        target.as_str(),
        DBUS_DEVICE_INTERFACE,
    )
    .await?;

    let mut events = proxy.receive_signal("InputEvent").await?;

    println!("Subscribed to {target}");

    while let Some(message) = events.next().await {
        let (event, value): (String, f64) = message.body().deserialize()?;

        if event == "ui_guide" && value > 0.5 {
            return Ok(target);
        }
    }

    Err(format!("D-Bus event stream closed: {target}").into())
}
