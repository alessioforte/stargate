use dotenv::dotenv;
use once_cell::sync::Lazy;
use std::env;
use surrealdb::engine::remote::ws::{Client, Ws};
// use surrealdb::opt::auth::Root;
use surrealdb::Surreal;

pub static DB: Lazy<Surreal<Client>> = Lazy::new(Surreal::init);

pub async fn init() {
    dotenv().ok();
    let address: String = env::var("DB_ADDRESS").unwrap_or_else(|_| "".to_string());
    let username: String = env::var("DB_USERNAME").unwrap_or_else(|_| "".to_string());
    let password: String = env::var("DB_PASSWORD").unwrap_or_else(|_| "".to_string());
    let namsapce: String = env::var("DB_NAMESPACE").unwrap_or_else(|_| "".to_string());
    let database: String = env::var("DB_DATABASE").unwrap_or_else(|_| "".to_string());

    println!(
        "{} {} {} {} {}",
        address, username, password, namsapce, database
    );

    let connect = DB.connect::<Ws>(address).await;
    println!("connect {:?}", connect);

    // let signed = DB.signin(Root {
    //     username: &username,
    //     password: &password,
    // });
    // signed.await.unwrap();

    let ns = DB.use_ns(namsapce).use_db(database);
    ns.await.unwrap();
}

// use once_cell::sync::Lazy;
// use surrealdb::{
//     engine::remote::ws::{Client, Ws},
//     opt::auth::Root,
//     Result, Surreal,
// };

// pub static DB: Lazy<Surreal<Client>> = Lazy::new(Surreal::init);

// pub async fn connect_db() -> Result<()> {
//     let _ = DB.connect::<Ws>("localhost:8000").await?;
//     let _ = DB
//         .signin(Root {
//             username: "root",
//             password: "root",
//         })
//         .await;
//     let _ = DB.use_ns("todo").use_db("todo").await?;
//     Ok(())
// }
