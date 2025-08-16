use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};

pub struct Hash {}

impl Hash {
    /// Encode a string using Argon2 hashing algorithm.
    /// # Arguments
    /// * `value` - The string to be hashed.
    /// # Returns
    /// * `Result<String, argon2::password_hash::Error>` - The hashed string on success, or an error on failure.
    /// /// # Example
    /// ```rust
    /// use password::Hash;
    /// 
    /// let hashed_value = Hash::encode("my password").unwrap();
    /// let is_valid = Hash::verify("my password", &hashed_value).is_ok();
    /// 
    /// assert!(is_valid);
    /// ```
    pub fn encode(value: &str) -> Result<String, argon2::password_hash::Error> {
        let value = value.as_bytes();
        let salt = SaltString::generate(&mut OsRng);
        let argon2 = Argon2::default();
        let password_hash = argon2.hash_password(value, &salt)?.to_string();
        Ok(password_hash)
    }

    pub fn verify(value: &str, hash: &str) -> Result<(), argon2::password_hash::Error> {
        let value = value.as_bytes();
        let hash = PasswordHash::new(hash)?;
        let argon2 = Argon2::default();
        argon2.verify_password(value, &hash)
    }
}
