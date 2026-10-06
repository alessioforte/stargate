use super::{
    Gate, GatewayPreparationError,
    config::{get_policies_path, load_config_from_path},
};
use crate::etc::observability::telemetry;
use notify::{EventKind, RecursiveMode, Watcher, event::ModifyKind};
use std::{path::Path, sync::Arc, thread};
use tokio::runtime::Handle;
use tracing::{error, info};

fn file_content(path: &str) -> Option<Vec<u8>> {
    std::fs::read(path).ok()
}

fn is_write_event(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Modify(ModifyKind::Data(_)) | EventKind::Create(_)
    )
}

fn create_file_watcher(
    file_path: &str,
    label: &'static str,
    tx: std::sync::mpsc::Sender<notify::Result<notify::Event>>,
) -> Option<notify::RecommendedWatcher> {
    let mut watcher = match notify::recommended_watcher(tx) {
        Ok(watcher) => watcher,
        Err(error) => {
            error!(
                path = file_path,
                watcher = label,
                error = ?error,
                "Failed to initialize file watcher; live reload disabled"
            );
            return None;
        }
    };

    if let Err(error) = watcher.watch(Path::new(file_path), RecursiveMode::NonRecursive) {
        error!(
            path = file_path,
            watcher = label,
            error = ?error,
            "Failed to register file watcher; live reload disabled"
        );
        return None;
    }

    info!(
        path = file_path,
        watcher = label,
        "Watching file for changes"
    );
    Some(watcher)
}

pub(super) fn watch_config_file(file_path: &str, gate: &Arc<Gate>) {
    let file_path = file_path.to_string();
    let gate = gate.clone();
    let handle = Handle::current();
    thread::spawn(move || {
        handle.block_on(async {
            let (tx, rx) = std::sync::mpsc::channel();

            let mut last_content = file_content(&file_path);

            let Some(_watcher) = create_file_watcher(&file_path, "gate_config", tx) else {
                return;
            };

            for rs in rx {
                match rs {
                    Ok(event) => {
                        if !is_write_event(&event.kind) {
                            continue;
                        }
                        let current_content = file_content(&file_path);
                        if current_content == last_content {
                            continue;
                        }
                        match reload_gateway_config_from_path(&gate, &file_path) {
                            Ok(config_version) => {
                                last_content = current_content;
                                info!(config_version, "Configuration file changed, reloaded");
                            }
                            Err(error) => {
                                error!(%error, "Configuration file changed but did not validate; keeping previous config");
                            }
                        }
                    }
                    Err(e) => {
                        telemetry::record_config_reload("gateway_config", "error");
                        error!("Watch error: {:?}", e);
                    }
                }
            }
        });
    });
}

pub(crate) fn reload_gateway_config_from_path(
    gate: &Gate,
    path: &str,
) -> Result<u64, GatewayPreparationError> {
    let result = load_config_from_path(path).and_then(|prepared| gate.activate(prepared));
    telemetry::record_config_reload(
        "gateway_config",
        result
            .as_ref()
            .map_or_else(|error| error.reload_outcome(), |_| "success"),
    );
    result
}

pub(super) fn watch_policies_file(file_path: &str, gate: &Arc<Gate>) {
    let file_path = file_path.to_string();
    let gate = gate.clone();
    let handle = Handle::current();
    thread::spawn(move || {
        handle.block_on(async {
            let (tx, rx) = std::sync::mpsc::channel();

            let mut last_content = file_content(&file_path);

            let Some(_watcher) = create_file_watcher(&file_path, "policies", tx) else {
                return;
            };

            for rs in rx {
                match rs {
                    Ok(event) => {
                        if !is_write_event(&event.kind) {
                            continue;
                        }
                        let current_content = file_content(&file_path);
                        if current_content == last_content {
                            continue;
                        }
                        last_content = current_content;

                        info!("Policies file changed, reloading...");
                        if let Err(error) = reload_policy_engine_from_path(&gate, &file_path) {
                            telemetry::record_config_reload("policies", "error");
                            error!(%error, "Policy file changed but did not validate; keeping previous policies");
                        }
                    }
                    Err(e) => {
                        telemetry::record_config_reload("policies", "error");
                        error!("Watch error: {:?}", e);
                    }
                }
            }
        });
    });
}

pub fn reload_policy_engine(gate: &Gate) -> Result<(), crate::err::ErrorResponse> {
    let policies_path = get_policies_path();
    reload_policy_engine_from_path(gate, &policies_path).map(|_| ())
}

fn reload_policy_engine_from_path(
    gate: &Gate,
    policies_path: &str,
) -> Result<bool, crate::err::ErrorResponse> {
    let policies = crate::act::access_control_rules::load_policy_snapshot(policies_path)?;
    if gate.policy_snapshot.load().revision == policies.revision {
        return Ok(false);
    }

    gate.policy_snapshot.store(Arc::new(policies));
    telemetry::record_config_reload("policies", "success");
    Ok(true)
}

#[cfg(all(test, feature = "memory"))]
mod tests;
