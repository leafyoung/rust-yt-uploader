//! Authentication module for YouTube API using OAuth 2.0.
//!
//! This module provides OAuth 2.0 authentication (a hand-rolled Rust equivalent of
//! Python's google-auth-oauthlib) with PKCE support. It handles:
//! - Loading existing credentials from token files
//! - Token validation (expiration, scopes)
//! - Automatic token refresh
//! - Interactive OAuth flow with InstalledFlow pattern
//! - Secure token persistence
//! - PKCE (Proof Key for Code Exchange) for security

pub mod credentials;
pub mod google;
pub mod oauth;

pub use credentials::Credentials;
pub use google::GoogleOAuth;
