use serde::{Deserialize, Serialize};
use surrealdb::sql::Thing;

#[derive(Debug, Serialize, Deserialize)]
pub struct Record {
    #[serde(deserialize_with = "thing_to_string")]
    id: String,
}

pub fn thing_to_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let thing = Thing::deserialize(deserializer)?;
    let binding = thing.to_string();
    let parts = binding.split(":").collect::<Vec<&str>>();
    let id = parts.last().unwrap().to_string();
    Ok(id)
}
