use actix_web::web::Data;
use gate::Gate;
use std::{env, path::Path, sync::Arc};

pub fn init(store: Arc<lim::State>) -> Data<Gate> {
    let path = env::var("CONFIG_PATH").unwrap_or_else(|_| ".stargate".to_string());
    let filename = env::var("CONFIG_FILENAME").unwrap_or_else(|_| "config.yaml".to_string());
    if !Path::new(&path).exists() {
        std::fs::create_dir(&path).expect("Unable to create config directory");
    }
    let config_file_path = format!("{}/{}", path, filename);

    let config = gate::cfg::Config::from_file(&config_file_path);
    let gate = Gate::new(store).build(&config);
    gate.watch_file(&config_file_path);

    Data::new(gate)
}
