use crate::models::users::{Payload as UserPayload, User};
use crate::modules::hash::Hash;
use rand::Rng;

pub fn generate_password(
    length: usize,
    use_upper: bool,
    use_lower: bool,
    use_digits: bool,
    use_special: bool,
) -> String {
    let upper = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let lower = "abcdefghijklmnopqrstuvwxyz";
    let digits = "0123456789";
    let special = "!@#$%^&*()-_=+[]{}|;:,.<>?";

    let mut charset = String::new();
    if use_upper {
        charset.push_str(upper);
    }
    if use_lower {
        charset.push_str(lower);
    }
    if use_digits {
        charset.push_str(digits);
    }
    if use_special {
        charset.push_str(special);
    }

    let mut rng = rand::thread_rng();
    let password: String = (0..length)
        .map(|_| {
            let idx = rng.gen_range(0..charset.len());
            charset.chars().nth(idx).unwrap()
        })
        .collect();
    password
}

pub async fn create_super_admin() {
    let password = generate_password(40, true, true, true, false);

    // verify if super admin already exists
    let super_admin = User::get_by_email("admin@localhost".to_string())
        .await
        .unwrap();
    if super_admin.is_some() {
        return;
    }

    let super_admin = UserPayload {
        email: "admin@localhost".to_string(),
        name: "Admin".to_string(),
        nickname: Some("admin".to_string()),
        password: Hash::encode(&password).unwrap(),
        phone_number: None,
        picture: None,
    };

    log::info!("Super admin password: {}", password);
    User::create(super_admin).await.unwrap();
}
