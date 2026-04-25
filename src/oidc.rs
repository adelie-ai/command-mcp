#![deny(warnings)]

// OIDC discovery and JWKS verification

use crate::error::{Result, TransportError};
use jwtk::jwk::RemoteJwksVerifier;
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::RwLock;

/// OIDC well-known configuration response
#[derive(Debug, Clone, Deserialize)]
struct OidcConfig {
    /// JWKS URI endpoint
    #[serde(rename = "jwks_uri")]
    jwks_uri: String,
    /// Issuer identifier
    issuer: String,
}

/// JWKS verifier that handles OIDC discovery and key caching
pub struct JwksVerifier {
    /// JWKS URL (from OIDC discovery or direct configuration)
    jwks_url: String,
    /// Cached verifier instance
    verifier: Arc<RwLock<Option<Arc<RemoteJwksVerifier>>>>,
}

impl JwksVerifier {
    /// Create a new JWKS verifier from an OIDC issuer URL
    /// This will perform OIDC discovery to find the JWKS endpoint
    pub async fn from_oidc_issuer(issuer_url: &str) -> Result<Self> {
        // Normalize issuer URL (ensure it doesn't end with /)
        let issuer_url = issuer_url.trim_end_matches('/');

        // Construct well-known configuration URL
        let well_known_url = format!("{}/.well-known/openid-configuration", issuer_url);

        // Fetch OIDC configuration
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(|e| {
                TransportError::Authentication(format!("Failed to create HTTP client: {}", e))
            })?;

        let config: OidcConfig = client
            .get(&well_known_url)
            .send()
            .await
            .map_err(|e| {
                TransportError::Authentication(format!("Failed to fetch OIDC config: {}", e))
            })?
            .error_for_status()
            .map_err(|e| {
                TransportError::Authentication(format!("OIDC config request failed: {}", e))
            })?
            .json()
            .await
            .map_err(|e| {
                TransportError::Authentication(format!("Failed to parse OIDC config: {}", e))
            })?;

        // Verify issuer matches (normalize both for comparison)
        let config_issuer = config.issuer.trim_end_matches('/');
        if config_issuer != issuer_url {
            return Err(TransportError::Authentication(format!(
                "Issuer mismatch: expected {}, got {}",
                issuer_url, config_issuer
            ))
            .into());
        }

        Ok(Self {
            jwks_url: config.jwks_uri,
            verifier: Arc::new(RwLock::new(None)),
        })
    }

    /// Create a new JWKS verifier directly from a JWKS URL
    pub fn from_jwks_url(jwks_url: &str) -> Self {
        Self {
            jwks_url: jwks_url.to_string(),
            verifier: Arc::new(RwLock::new(None)),
        }
    }

    /// Get or create the verifier instance (lazy initialization)
    async fn get_verifier(&self) -> Result<Arc<RemoteJwksVerifier>> {
        // Check if verifier is already initialized
        {
            let verifier = self.verifier.read().await;
            if let Some(ref v) = *verifier {
                return Ok(Arc::clone(v));
            }
        }

        let verifier = RemoteJwksVerifier::builder(self.jwks_url.clone())
            .with_cache_duration(std::time::Duration::from_secs(3600))
            .build();

        let verifier = Arc::new(verifier);

        // Store in cache
        {
            let mut cache = self.verifier.write().await;
            *cache = Some(Arc::clone(&verifier));
        }

        Ok(verifier)
    }

    /// Verify a JWT token
    pub async fn verify(&self, token: &str) -> Result<jwtk::HeaderAndClaims<serde_json::Value>> {
        let verifier = self.get_verifier().await?;

        // RemoteJwksVerifier has its own verify method (which is async)
        verifier.verify(token).await.map_err(|e| {
            TransportError::Authentication(format!("JWT verification failed: {}", e)).into()
        })
    }
}

#[cfg(test)]
mod tests {
    // Note: These tests would require a mock OIDC server or test fixtures
    // For now, we'll skip integration tests and focus on unit tests for the logic

    #[tokio::test]
    #[ignore] // Requires actual OIDC server
    async fn test_oidc_discovery() {
        // This would test against a real OIDC provider like Keycloak or Auth0
        // For now, we skip it
    }
}
