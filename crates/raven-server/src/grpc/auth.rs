use tonic::{Request, Status};

/// Extracts the Bearer token from gRPC request metadata
pub fn extract_bearer_token<T>(request: &Request<T>) -> Result<String, Status> {
    let header = request
        .metadata()
        .get("authorization")
        .ok_or_else(|| Status::unauthenticated("missing authorization header"))?;

    let value = header
        .to_str()
        .map_err(|_| Status::unauthenticated("authorization header contains invalid characters"))?;

    value
        .strip_prefix("Bearer ")
        .map(|s| s.to_string())
        .ok_or_else(|| Status::unauthenticated("authorization header must use Bearer scheme"))
}
