//! Shared configuration for the admin panel backend.

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub database: DatabaseConfig,
    pub server: ServerConfig,
    pub auth: AuthConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    pub url: String,
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,
}

fn default_max_connections() -> u32 {
    10
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub address: String,
    pub port: u16,
    /// CORS allowlist for consumer frontends fetching runtime branding.
    pub cors_allowed_origins: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AuthConfig {
    /// Trusted JWKS origin of the central auth server.
    pub jwks_uri: String,
    pub issuer: String,
    pub audience: String,
}

impl AppConfig {
    pub fn from_env() -> Result<Self, config::ConfigError> {
        let builder = config::Config::builder()
            .set_default("database.max_connections", 10u64)?
            .set_default("server.address", "0.0.0.0")?
            .set_default("server.port", "7771")?
            .set_default("server.cors_allowed_origins", Vec::<String>::new())?
            .set_default("auth.jwks_uri", "http://127.0.0.1:7701/oidc/jwks")?
            .set_default("auth.issuer", "http://127.0.0.1:7701")?
            .set_default("auth.audience", "sdlc")?;
        let _ = builder.build()?;
        let database_url = std::env::var("ADMINP_DATABASE_URL").unwrap_or_default();
        if database_url.is_empty() {
            return Err(config::ConfigError::Message(
                "ADMINP_DATABASE_URL is required".into(),
            ));
        }
        let port: u16 = std::env::var("ADMINP_BIND_PORT")
            .unwrap_or_else(|_| "7771".into())
            .parse()
            .map_err(|_| config::ConfigError::Message("invalid ADMINP_BIND_PORT".into()))?;
        let cors_allowed_origins = std::env::var("ADMINP_CORS_ALLOWED_ORIGINS")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect::<Vec<_>>();
        Ok(Self {
            database: DatabaseConfig {
                url: database_url,
                max_connections: 10,
            },
            server: ServerConfig {
                address: std::env::var("ADMINP_BIND_ADDRESS").unwrap_or("0.0.0.0".into()),
                port,
                cors_allowed_origins,
            },
            auth: AuthConfig {
                jwks_uri: std::env::var("ADMINP_AUTH_JWKS_URI")
                    .unwrap_or("http://127.0.0.1:7701/oidc/jwks".into()),
                issuer: std::env::var("ADMINP_AUTH_ISSUER")
                    .unwrap_or("http://127.0.0.1:7701".into()),
                audience: std::env::var("ADMINP_AUTH_AUDIENCE").unwrap_or("sdlc".into()),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_port_from_string_env() {
        let port: u16 = "7771".parse().unwrap();
        assert_eq!(port, 7771);
    }
}
