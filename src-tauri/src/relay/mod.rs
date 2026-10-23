//! Everything this product does against the Aimyria relay.
//!
//! # Why a module instead of one more `services::*` file
//!
//! The desktop app is being wired to the console: login, keys, billing, usage,
//! model status. Those are one product surface with one set of rules — which
//! hosts may be contacted, which credential may travel to which endpoint, what a
//! failure is allowed to look like on screen. Keeping that in one place is what
//! makes those rules auditable; spreading it across command modules is how a
//! session token eventually leaks into a log line.
//!
//! See `docs/relay-integration-plan.md` for the staged plan this module serves.
//!
//! # The two credentials, and why they are never interchangeable
//!
//! * **API key** (`sk-…`) — authenticates a *client* to the model gateway. It can
//!   read its own balance and call models. It cannot manage the account.
//! * **Device session** — authenticates the *desktop app* to the console so the
//!   user can manage keys, billing and tickets without opening a browser.
//!
//! Sending an API key where a session belongs gets a 401 that reads like "wrong
//! password"; sending a session where a key belongs leaks account authority to a
//! model endpoint. [`Credential`] exists so a call site has to say which one it
//! means, and [`Endpoint`] decides whether that pairing is allowed at all.

pub mod client;
pub mod hosts;
pub mod keys;
pub mod session;
pub mod state;

#[cfg(test)]
mod refresh_e2e;

pub use client::{RelayClient, RelayError};
pub use hosts::{is_relay_endpoint, is_relay_host, RelayHost, ALL_HOSTS};
pub use state::{SessionState, SessionStatus, SharedSession};

/// Unix seconds.
///
/// One helper so every expiry comparison in this module uses the same clock and
/// the same unit. Mixing `SystemTime` with `chrono` is how a "token expires in an
/// hour" turns into "expired in 1970".
pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// What is being authorised on a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Credential<'a> {
    /// A user's model-gateway key. Only valid for gateway endpoints.
    ApiKey(&'a str),
    /// The desktop app's console session token. Only valid for console endpoints.
    Session(&'a str),
}

impl<'a> Credential<'a> {
    /// The bearer value, if there is one.
    ///
    /// Returns `None` for an empty credential rather than an empty header: a
    /// `Authorization: Bearer ` header is a malformed request on some gateways and
    /// an anonymous one on others, and neither is what the caller meant.
    pub fn bearer(self) -> Option<&'a str> {
        let value = match self {
            Credential::ApiKey(value) | Credential::Session(value) => value,
        };
        let trimmed = value.trim();
        (!trimmed.is_empty()).then_some(trimmed)
    }

    /// Is this credential allowed to be sent to this endpoint?
    ///
    /// The rule is deliberately narrow. Widening it is a security change, not a
    /// convenience one — see the module docs.
    pub fn allowed_for(self, endpoint: Endpoint) -> bool {
        match (self, endpoint) {
            (Credential::ApiKey(_), Endpoint::Gateway) => true,
            (Credential::Session(_), Endpoint::Console) => true,
            // Everything else is a category error, which is exactly the bug this
            // type is here to make unrepresentable.
            _ => false,
        }
    }
}

/// Which family of endpoint is being called.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Endpoint {
    /// Model gateway: `/v1/models`, `/v1/user/balance`, model traffic.
    Gateway,
    /// Console management API: keys, billing, usage, tickets.
    Console,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_api_key_may_only_reach_the_gateway() {
        let key = Credential::ApiKey("sk-abc");
        assert!(key.allowed_for(Endpoint::Gateway));
        assert!(
            !key.allowed_for(Endpoint::Console),
            "an API key must never authorise console management"
        );
    }

    #[test]
    fn a_session_may_only_reach_the_console() {
        let session = Credential::Session("session-token");
        assert!(session.allowed_for(Endpoint::Console));
        assert!(
            !session.allowed_for(Endpoint::Gateway),
            "a session token must never be sent to a model endpoint"
        );
    }

    #[test]
    fn an_empty_credential_produces_no_header() {
        assert_eq!(Credential::ApiKey("").bearer(), None);
        assert_eq!(Credential::ApiKey("   ").bearer(), None);
        assert_eq!(Credential::Session("").bearer(), None);
    }

    #[test]
    fn a_present_credential_is_trimmed() {
        assert_eq!(Credential::ApiKey("  sk-abc  ").bearer(), Some("sk-abc"));
    }
}
