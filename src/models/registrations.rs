use crate::db::DB;
use crate::models::records::Record;
use serde::{Deserialize, Serialize};

use crate::models::records::thing_to_string;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewRegistration {
    pub email: String,
    pub uuid: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Registration {
    #[serde(deserialize_with = "thing_to_string")]
    pub id: String,
    pub email: String,
    pub uuid: String,
}

impl Registration {
    pub async fn create(registration: NewRegistration) -> surrealdb::Result<Vec<Record>> {
        DB.create("registrations").content(registration).await
    }

    pub async fn get_by_email(email: String) -> surrealdb::Result<Option<Registration>> {
        let sql = "SELECT * FROM registrations WHERE email = $email";
        let mut response = DB.query(sql).bind(("email", email)).await?;
        let registrations: Vec<Registration> = response.take(0)?;
        let registration = registrations.first().cloned();
        Ok(registration)
    }

    pub async fn get_by_id(id: String) -> surrealdb::Result<Option<Registration>> {
        let sql = "SELECT * FROM registrations WHERE uuid = $uuid";
        let mut response = DB.query(sql).bind(("uuid", id)).await?;
        let registrations: Vec<Registration> = response.take(0)?;
        let registration = registrations.first().cloned();
        Ok(registration)
    }

    pub async fn delete(id: String) -> surrealdb::Result<Option<Registration>> {
        let id = id.split(":").collect::<Vec<&str>>()[1];
        DB.delete(("registrations", id)).await
    }
}



// use std::fmt;

// #[derive(Debug)]
// pub enum RegistrationError {
//     DatabaseError(surrealdb::Error),
//     InvalidId,
// }

// impl fmt::Display for RegistrationError {
//     fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
//         match self {
//             Self::DatabaseError(e) => write!(f, "Database error: {}", e),
//             Self::InvalidId => write!(f, "Invalid ID"),
//         }
//     }
// }

// impl From<surrealdb::Error> for RegistrationError {
//     fn from(err: surrealdb::Error) -> RegistrationError {
//         RegistrationError::DatabaseError(err)
//     }
// }

// impl Registration {
//     pub async fn create(registration: NewRegistration) -> Result<Vec<Record>, RegistrationError> {
//         DB.create("registrations").content(registration).await.map_err(RegistrationError::from)
//     }

//     pub async fn get_by_email(email: String) -> Result<Option<Registration>, RegistrationError> {
//         let sql = "SELECT * FROM registrations WHERE email = $email";
//         let mut response = DB.query(sql).bind(("email", email)).await.map_err(RegistrationError::from)?;
//         let registrations: Vec<Registration> = response.take(0)?;
//         Ok(registrations.first().cloned())
//     }

//     pub async fn get_by_id(id: String) -> Result<Option<Registration>, RegistrationError> {
//         let sql = "SELECT * FROM registrations WHERE uuid = $uuid";
//         let mut response = DB.query(sql).bind(("uuid", id)).await.map_err(RegistrationError::from)?;
//         let registrations: Vec<Registration> = response.take(0)?;
//         Ok(registrations.first().cloned())
//     }

//     pub async fn delete(id: String) -> Result<Option<Registration>, RegistrationError> {
//         let id_parts: Vec<&str> = id.split(":").collect();
//         if id_parts.len() < 2 {
//             return Err(RegistrationError::InvalidId);
//         }
//         DB.delete(("registrations", id_parts[1])).await.map_err(RegistrationError::from)
//     }
// }