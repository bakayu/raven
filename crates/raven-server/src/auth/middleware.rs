use axum::{
    extract::FromRequestParts,
    http::{header::AUTHORIZATION, request::Parts},
};

use crate::{
    auth::jwt::{self, Claims},
    error::AppError,
    state::AppState,
};

pub struct RequireAuth(pub Claims);

impl FromRequestParts<AppState> for RequireAuth {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let auth_header = parts
            .headers
            .get(AUTHORIZATION)
            .ok_or(AppError::Unauthorized)?;

        let auth_str = auth_header.to_str().map_err(|_| AppError::Unauthorized)?;
        if !auth_str.starts_with("Bearer ") {
            return Err(AppError::Unauthorized);
        }

        let token = &auth_str["Bearer ".len()..];
        let claims = jwt::validate(
            token,
            &state.config.auth.jwt_issuer,
            &state.config.auth.jwt_audience,
            secrecy::ExposeSecret::expose_secret(&state.config.auth.jwt_signing_key),
        )?;

        Ok(RequireAuth(claims))
    }
}

pub struct RequireAdmin(pub Claims);

impl FromRequestParts<AppState> for RequireAdmin {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let RequireAuth(claims) = RequireAuth::from_request_parts(parts, state).await?;

        if claims.role != "admin" {
            return Err(AppError::Forbidden);
        }

        Ok(RequireAdmin(claims))
    }
}
