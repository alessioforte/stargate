use actix_web::web::Data;
use gate::Gate;
use std::{env, path::Path};

pub fn init() -> Data<Gate> {
    let path = env::var("CONFIG_PATH").unwrap_or_else(|_| ".stargate".to_string());
    let filename = env::var("CONFIG_FILENAME").unwrap_or_else(|_| "config.yaml".to_string());
    if !Path::new(&path).exists() {
        std::fs::create_dir(&path).expect("Unable to create config directory");
    }
    let file_path = format!("{}/{}", path, filename);

    let gate = Gate::new(file_path).build();
    gate.watch_file();

    Data::new(gate)
}
