use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng},
};

use crate::error::AppError;
use crate::error::AppResult;

pub fn hash(password: &str) -> AppResult<String> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(AppError::PasswordHash)?;
    Ok(hash.to_string())
}

pub fn verify(password: &str, hash: &str) -> AppResult<bool> {
    let parsed = PasswordHash::new(hash).map_err(AppError::PasswordHash)?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_verify_roundtrip() {
        let pwd = "super-secret";
        let hashed = hash(pwd).expect("hash password");
        assert_ne!(hashed, pwd);

        let ok = verify(pwd, &hashed).expect("verify password");
        assert!(ok);
    }

    #[test]
    fn verify_returns_false_for_wrong_password() {
        let hashed = hash("right-password").expect("hash password");
        let ok = verify("wrong-password", &hashed).expect("verify password");
        assert!(!ok);
    }

    #[test]
    fn verify_errors_for_invalid_hash_format() {
        let err = verify("whatever", "not-a-valid-argon2-hash").expect_err("should fail");
        match err {
            crate::error::AppError::PasswordHash(_) => {}
            other => panic!("unexpected error: {other}"),
        }
    }
}
