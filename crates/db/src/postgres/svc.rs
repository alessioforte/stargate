use super::repo::{ActionRepository, CredentialRepository, UserRepository};
use crate::ent::{Action, ActionType, Credential, CredentialType, User};
use crate::tx::Transaction;
use anyhow::Result;
use sqlx::postgres::{PgPool, PgPoolOptions};
use std::fs;

#[derive(Clone)]
pub struct Service {
    pool: PgPool,
    user: UserRepository,
    credential: CredentialRepository,
    action: ActionRepository,
}

impl Service {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            user: UserRepository::new(),
            credential: CredentialRepository::new(),
            action: ActionRepository::new(),
        }
    }

    // TODO: handle with migrations
    pub async fn init_schema(&self) {
        let ddl = fs::read_to_string("ddl/postgres.sql").expect("Failed to read schema file");
        for stmt in ddl.split(';') {
            if !stmt.trim().is_empty() {
                sqlx::query(stmt)
                    .execute(&self.pool)
                    .await
                    .expect("Failed to execute schema statement");
            }
        }
    }
}

pub async fn init(conn: &str) -> Result<Service> {
    let pool = PgPoolOptions::new().connect(conn).await?;
    let service = Service::new(pool.clone());
    service.init_schema().await;
    Ok(service)
}

#[async_trait::async_trait]
impl Transaction for Service {
    async fn create_user(
        &self,
        user: User,
        credential_type: CredentialType,
        value: &str,
    ) -> Result<User> {
        let mut tx = self.pool.begin().await?;
        let user_id = user.id.clone();
        let record = self.user.create(&mut tx, user).await?;
        self.credential
            .create(&mut tx, &user_id, credential_type, value)
            .await?;
        tx.commit().await?;
        Ok(record)
    }

    async fn get_user_by_username(&self, username: &str) -> Result<Option<User>> {
        let mut tx = self.pool.begin().await?;
        let user = self.user.get_by_username(&mut tx, username).await?;
        tx.commit().await?;
        Ok(user)
    }

    async fn update_user(&self, user: User) -> Result<User> {
        let mut tx = self.pool.begin().await?;
        let updated_user = self.user.update(&mut tx, user).await?;
        tx.commit().await?;
        Ok(updated_user)
    }

    async fn change_password(&self, user_id: &str, new_password: &str) -> Result<Credential> {
        let mut tx = self.pool.begin().await?;
        let credential = self
            .credential
            .change_password(&mut tx, user_id, new_password)
            .await?;
        tx.commit().await?;
        Ok(credential)
    }

    async fn get_credential(
        &self,
        user_id: &str,
        credential_type: CredentialType,
    ) -> Result<Option<Credential>> {
        let mut tx = self.pool.begin().await?;
        let credential = self
            .credential
            .get_by_user_id(&mut tx, user_id, credential_type)
            .await?;
        tx.commit().await?;
        Ok(credential)
    }

    async fn create_action(&self, action: Action) -> Result<Action> {
        let mut tx = self.pool.begin().await?;
        let record = self.action.create(&mut tx, action).await?;
        tx.commit().await?;
        Ok(record)
    }

    async fn get_action_by_value(&self, value: &str) -> Result<Option<Action>> {
        let mut tx = self.pool.begin().await?;
        let action = self.action.get_by_value(&mut tx, value).await?;
        tx.commit().await?;
        Ok(action)
    }

    async fn get_action_by_sub_and_type(
        &self,
        sub: &str,
        action_type: ActionType,
    ) -> Result<Option<Action>> {
        let mut tx = self.pool.begin().await?;
        let action = self
            .action
            .get_by_sub_and_type(&mut tx, sub, action_type)
            .await?;
        tx.commit().await?;
        Ok(action)
    }
}

// use super::repo::{ActionRepository, CredentialRepository, UserRepository};
// use crate::ent::{Action, ActionType, Credential, CredentialType, User};
// use crate::svc::Transaction;
// use anyhow::Result;
// use std::fs;

// #[derive(Clone)]
// pub struct Service {
//     pool: sqlx::AnyPool,
//     user: UserRepository,
//     credential: CredentialRepository,
//     action: ActionRepository,
// }

// impl Service {
//     pub fn new(pool: sqlx::AnyPool) -> Self {
//         Self {
//             pool,
//             user: UserRepository::new(),
//             credential: CredentialRepository::new(),
//             action: ActionRepository::new(),
//         }
//     }

//     // TODO: handle with migrations
//     pub async fn init_schema(&self) {
//         let ddl = fs::read_to_string("ddl/sqlite.sql").expect("Failed to read schema file");
//         for stmt in ddl.split(';') {
//             if !stmt.trim().is_empty() {
//                 sqlx::query(stmt)
//                     .execute(&self.pool)
//                     .await
//                     .expect("Failed to execute schema statement");
//             }
//         }
//     }
// }

// pub async fn init(conn: &str) -> Result<Service> {
//     sqlx::any::install_default_drivers();
//     let pool = sqlx::AnyPool::connect(conn)
//         .await
//         .map_err(|e| anyhow::anyhow!("Failed to connect to database: {}", e))?;
//     let service = Service::new(pool.clone());
//     service.init_schema().await;
//     Ok(service)
// }

// #[async_trait::async_trait]
// impl Transaction for Service {
//     async fn create_user(
//         &self,
//         user: User,
//         credential_type: CredentialType,
//         value: &str,
//     ) -> Result<User> {
//         let mut tx = self.pool.begin().await?;
//         let user_id = user.id.clone();
//         let record = self.user.create(&mut tx, user).await?;
//         self.credential
//             .create(&mut tx, &user_id, credential_type, value)
//             .await?;
//         tx.commit().await?;
//         Ok(record)
//     }

//     async fn get_user_by_username(&self, username: &str) -> Result<Option<User>> {
//         let mut tx = self.pool.begin().await?;
//         let user = self.user.get_by_username(&mut tx, username).await?;
//         tx.commit().await?;
//         Ok(user)
//     }

//     async fn update_user(&self, user: User) -> Result<User> {
//         let mut tx = self.pool.begin().await?;
//         let updated_user = self.user.update(&mut tx, user).await?;
//         tx.commit().await?;
//         Ok(updated_user)
//     }

//     async fn change_password(&self, user_id: &str, new_password: &str) -> Result<Credential> {
//         let mut tx = self.pool.begin().await?;
//         let credential = self
//             .credential
//             .change_password(&mut tx, user_id, new_password)
//             .await?;
//         tx.commit().await?;
//         Ok(credential)
//     }

//     async fn get_credential(
//         &self,
//         user_id: &str,
//         credential_type: CredentialType,
//     ) -> Result<Option<Credential>> {
//         let mut tx = self.pool.begin().await?;
//         let credential = self
//             .credential
//             .get_by_user_id(&mut tx, user_id, credential_type)
//             .await?;
//         tx.commit().await?;
//         Ok(credential)
//     }

//     async fn create_action(&self, action: Action) -> Result<Action> {
//         let mut tx = self.pool.begin().await?;
//         let record = self.action.create(&mut tx, action).await?;
//         tx.commit().await?;
//         Ok(record)
//     }

//     async fn get_action_by_value(&self, value: &str) -> Result<Option<Action>> {
//         let mut tx = self.pool.begin().await?;
//         let action = self.action.get_by_value(&mut tx, value).await?;
//         tx.commit().await?;
//         Ok(action)
//     }

//     async fn get_action_by_sub_and_type(
//         &self,
//         sub: &str,
//         action_type: ActionType,
//     ) -> Result<Option<Action>> {
//         let mut tx = self.pool.begin().await?;
//         let action = self
//             .action
//             .get_by_sub_and_type(&mut tx, sub, action_type)
//             .await?;
//         tx.commit().await?;
//         Ok(action)
//     }
// }
