use chrono::Utc;
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};

use crate::error::AppError;

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String, // user id
    pub username: String,
    pub role: String,
    pub iss: String,
    pub aud: String,
    pub exp: i64,
    pub iat: i64,
}

pub fn issue(
    user_id: &str,
    username: &str,
    role: &str,
    issuer: &str,
    audience: &str,
    ttl_minutes: i64,
    secret: &str,
) -> Result<String, AppError> {
    let now = Utc::now();
    let claims = Claims {
        sub: user_id.to_string(),
        username: username.to_string(),
        role: role.to_string(),
        iss: issuer.to_string(),
        aud: audience.to_string(),
        iat: now.timestamp(),
        exp: (now + chrono::Duration::minutes(ttl_minutes)).timestamp(),
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(AppError::Jwt)
}

pub fn validate(
    token: &str,
    issuer: &str,
    audience: &str,
    secret: &str,
) -> Result<Claims, AppError> {
    let mut validation = Validation::default();
    validation.set_issuer(&[issuer]);
    validation.set_audience(&[audience]);

    validation.leeway = 0;
    validation.validate_exp = true;
    validation.required_spec_claims.insert("exp".to_string());

    decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )
    .map(|d| d.claims)
    .map_err(AppError::Jwt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_and_validate_roundtrip() {
        let token = issue(
            "user-1",
            "alice",
            "admin",
            "raven",
            "raven-web",
            15,
            "secret-1",
        )
        .expect("issue token");

        let claims = validate(&token, "raven", "raven-web", "secret-1").expect("validate token");

        assert_eq!(claims.sub, "user-1");
        assert_eq!(claims.username, "alice");
        assert_eq!(claims.role, "admin");
        assert_eq!(claims.iss, "raven");
        assert_eq!(claims.aud, "raven-web");
        assert!(claims.exp > claims.iat);
    }

    #[test]
    fn validate_rejects_wrong_secret() {
        let token = issue(
            "user-1",
            "alice",
            "admin",
            "raven",
            "raven-web",
            15,
            "secret-1",
        )
        .expect("issue token");

        let err = validate(&token, "raven", "raven-web", "secret-2").expect_err("must fail");
        match err {
            crate::error::AppError::Jwt(_) => {}
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn validate_rejects_wrong_issuer() {
        let token = issue(
            "user-1",
            "alice",
            "admin",
            "raven",
            "raven-web",
            15,
            "secret-1",
        )
        .expect("issue token");

        let err = validate(&token, "wrong-issuer", "raven-web", "secret-1").expect_err("must fail");
        match err {
            crate::error::AppError::Jwt(_) => {}
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn validate_rejects_expired_token() {
        let token = issue(
            "user-1",
            "alice",
            "admin",
            "raven",
            "raven-web",
            -5,
            "secret-1",
        )
        .expect("issue token");

        let err = validate(&token, "raven", "raven-web", "secret-1").expect_err("must fail");
        match err {
            crate::error::AppError::Jwt(_) => {}
            other => panic!("unexpected error: {other}"),
        }
    }
}
