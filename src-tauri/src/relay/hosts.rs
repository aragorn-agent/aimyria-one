//! The one place that decides which hostnames this product will talk to.
//!
//! # Why this is not a `const` in each caller
//!
//! It already went wrong once, in a way users saw: the "is this client
//! configured?" check knew `api.aimyria.com` and `321ai.cc`, while the writers
//! emitted `aimyria.com` and `sub2.aimyria.com`. Two clients were configured and
//! serving traffic, and the one-click screen showed 「接入」 for both.
//!
//! The generalisation is not "add more strings" — it is that a host list checked
//! by `contains` and a host list written by three different writers will drift.
//! So: one enumeration, one matcher, and every caller goes through it.

use std::fmt;

/// A relay host this product is willing to contact.
///
/// Enumerated rather than free-form strings so a new endpoint in the plan
/// (`/desktop/v1/*`) cannot invent a host by accident.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelayHost {
    /// Model gateway. Serves `/v1/models`, `/v1/user/balance`, model traffic.
    Gateway,
    /// The console/site vhost. A saved provider imported from the console keeps
    /// this as its base URL; it serves the site, not the balance endpoint.
    Console,
    /// The sub-gateway a newer console import stores as the Codex base URL.
    Sub2,
    /// The legacy 321AI host. Installs written against it are configured, not
    /// broken, so it stays recognised.
    Legacy,
}

impl RelayHost {
    /// The canonical hostname for this role.
    pub fn hostname(self) -> &'static str {
        match self {
            RelayHost::Gateway => "api.aimyria.com",
            RelayHost::Console => "aimyria.com",
            RelayHost::Sub2 => "sub2.aimyria.com",
            RelayHost::Legacy => "321ai.cc",
        }
    }

    /// Hosts that are acceptable spellings of this role.
    ///
    /// `www.` is included because a user pasting a URL by hand writes it, and
    /// refusing the paste would be a false negative on a perfectly valid endpoint.
    fn aliases(self) -> &'static [&'static str] {
        match self {
            RelayHost::Console => &["aimyria.com", "www.aimyria.com"],
            RelayHost::Gateway => &["api.aimyria.com"],
            RelayHost::Sub2 => &["sub2.aimyria.com"],
            RelayHost::Legacy => &["321ai.cc", "www.321ai.cc"],
        }
    }

    /// Where a balance request for this host must go.
    ///
    /// Measured: `https://aimyria.com/v1/user/balance` is **404** while
    /// `https://api.aimyria.com/v1/user/balance` is **200**. Only the gateway
    /// answers the balance endpoint, so every one of our hosts resolves to it.
    pub fn balance_url(self) -> String {
        format!("https://{}/v1/user/balance", RelayHost::Gateway.hostname())
    }

    /// Single-label subdomains of this role that also belong to it.
    ///
    /// Added because the console hands out *sub*domains: this machine's WorkBuddy
    /// file points at `wb.321ai.cc`, the WorkBuddy surface of the 321AI deployment.
    /// An exact-match list missed it, so "is this one of ours?" answered **no** for
    /// a host we operate.
    ///
    /// That was wrong in two directions at once. The status panel would show a
    /// client as unconfigured while it was live, and `revert_to_official` treats
    /// "not ours" as "leave the record alone", so the revert silently did nothing.
    ///
    /// Only these labels are accepted, and only as `<label>.<hostname>` — a
    /// look-alike sharing the suffix (see [`is_relay_host`]) does not qualify.
    fn subdomains(self) -> &'static [&'static str] {
        match self {
            // `wb` is the WorkBuddy surface; `sub2` is kept for symmetry in case a
            // deployment serves it under the legacy domain.
            RelayHost::Legacy => &["wb", "sub2"],
            _ => &[],
        }
    }
}

impl fmt::Display for RelayHost {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.hostname())
    }
}

/// Every host this product recognises.
pub const ALL_HOSTS: [RelayHost; 4] = [
    RelayHost::Gateway,
    RelayHost::Console,
    RelayHost::Sub2,
    RelayHost::Legacy,
];

/// Classify a bare hostname.
///
/// Case-insensitive, and rejects look-alikes: `aimyria.com.evil.example` and
/// `notaimyria.com` are not ours, which a substring test cannot express.
pub fn is_relay_host(host: &str) -> Option<RelayHost> {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();

    // Exact spellings first: `api.aimyria.com` must classify as Gateway, not as a
    // subdomain of Console.
    if let Some(exact) = ALL_HOSTS
        .into_iter()
        .find(|candidate| candidate.aliases().iter().any(|alias| *alias == host))
    {
        return Some(exact);
    }

    // Then known role subdomains: `wb.321ai.cc`.
    ALL_HOSTS.into_iter().find(|candidate| {
        candidate.subdomains().iter().any(|label| {
            host
                .strip_prefix(label)
                .and_then(|rest| rest.strip_prefix('.'))
                .is_some_and(|domain| {
                    candidate.aliases().iter().any(|alias| *alias == domain)
                })
        })
    })
}

/// Classify a full endpoint (`scheme://host[:port][/path]`).
///
/// A bare `host/path` and a bare `host` are both accepted: configs store all
/// three shapes, and a matcher that only understood URLs would silently answer
/// "not ours" for a `.env` fragment.
pub fn is_relay_endpoint(endpoint: &str) -> Option<RelayHost> {
    let raw = endpoint.trim();
    if raw.is_empty() {
        return None;
    }

    // Loopback over plain http, only while the test switch is on.
    //
    // The acceptance suite drives the real refresh flow against a local server; a
    // production build must never accept `http://127.0.0.1` as a console base,
    // because that is a trivial local hijack. Same pattern (and same reason) as
    // `updater::TEST_CLOCK_ENV`: the shipping rules stay identical, and the escape
    // hatch is a named environment variable that no user sets.
    if test_clock_on() {
        if let Some(host) = loopback_console_host(raw) {
            return Some(host);
        }
    }

    // Find the scheme separator only if it is actually a scheme. An `.env` line
    // passed by mistake (`ANTHROPIC_BASE_URL=https://aimyria.com/v1`) also contains
    // `://`, and blindly splitting on it takes `ANTHROPIC_BASE_URL=https` for the
    // scheme and `aimyria.com` for the host — which *classifies successfully* and
    // would let a malformed request through as if it were valid. So the prefix must
    // look like a scheme: letters/digits/`+`/`-`/`.`, starting with a letter.
    let after_scheme = match raw.split_once("://") {
        Some((scheme, rest)) if is_scheme(scheme) => rest,
        // No scheme, or a prefix that is not one: treat the whole input as an
        // authority and let the host rules decide.
        _ => raw,
    };

    let authority = after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(after_scheme);
    // Strip userinfo, then the port, so `user@api.aimyria.com:443` classifies.
    let host_port = authority.rsplit('@').next().unwrap_or(authority);
    let host = match host_port.rsplit_once(':') {
        // A trailing `:port` is a port; a `:segment` with non-digits is not a host
        // shape we accept, and `is_relay_host` will reject it.
        Some((head, port)) if port.chars().all(|c| c.is_ascii_digit()) => head,
        _ => host_port,
    };
    is_relay_host(host)
}

/// Is the acceptance/troubleshooting switch on?
///
/// Named with the product prefix so it cannot collide with a generic `TEST` var
/// that a user's shell might already export.
pub const TEST_CLOCK_ENV: &str = "AIMYRIA_RELAY_TEST_CONSOLE";

fn test_clock_on() -> bool {
    std::env::var(TEST_CLOCK_ENV)
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

/// Accept `http://127.0.0.1:<port>` / `http://localhost:<port>` as a console host.
///
/// Only loopback, only plain http, only while the switch is on. Returns the
/// `Console` role so the rest of the client treats it exactly like the real one.
fn loopback_console_host(raw: &str) -> Option<RelayHost> {
    let rest = raw.strip_prefix("http://")?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    let host = match authority.rsplit_once(':') {
        Some((head, port)) if port.chars().all(|c| c.is_ascii_digit()) => head,
        _ => authority,
    };
    let is_loopback = host.eq_ignore_ascii_case("127.0.0.1")
        || host.eq_ignore_ascii_case("localhost")
        || host == "[::1]"
        || host == "::1";
    is_loopback.then_some(RelayHost::Console)
}

/// Does this look like a URL scheme (`https`, `http`, `aimyria`, …)?
fn is_scheme(candidate: &str) -> bool {
    let mut chars = candidate.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// Serialises tests that read or write the process-global acceptance switch.
///
/// `cargo test` runs tests in parallel threads of one process, so an
/// `env::set_var` in one test is visible to another mid-assertion. A mutex is the
/// only correct way to keep these honest; without it this file's two tests and the
/// `refresh_e2e` module would flake depending on scheduling.
///
/// Declared at module scope rather than inside `mod tests` because `refresh_e2e`
/// is a sibling module and needs the same lock.
#[cfg(test)]
pub(crate) static TEST_SWITCH_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_host_the_writers_emit_is_recognised() {
        for (endpoint, expected) in [
            ("https://api.aimyria.com", RelayHost::Gateway),
            ("https://api.aimyria.com/v1", RelayHost::Gateway),
            ("https://aimyria.com", RelayHost::Console),
            ("https://aimyria.com/v1", RelayHost::Console),
            ("https://www.aimyria.com/", RelayHost::Console),
            ("https://sub2.aimyria.com/v1", RelayHost::Sub2),
            ("https://321ai.cc/v1", RelayHost::Legacy),
        ] {
            assert_eq!(
                is_relay_endpoint(endpoint),
                Some(expected),
                "{endpoint} must classify as {expected}"
            );
        }
    }

    #[test]
    fn look_alike_domains_are_not_ours() {
        for endpoint in [
            "https://aimyria.com.evil.example",
            "https://notaimyria.com/v1",
            "https://api.aimyria.community/v1",
            "https://sub2.aimyria.community",
            "https://openrouter.ai/api/v1",
            "https://relay.example",
            // A subdomain of a look-alike is not ours either.
            "https://wb.not321ai.cc/v1",
            "https://wb.321ai.cc.evil.example/v1",
        ] {
            assert_eq!(
                is_relay_endpoint(endpoint),
                None,
                "{endpoint} must not be accepted"
            );
        }
    }

    /// Known role subdomains count as ours.
    ///
    /// `wb.321ai.cc` is the WorkBuddy surface of the 321AI deployment and appears
    /// in this machine's real `models.json`. Exact matching missed it, so the
    /// status panel called a live client unconfigured and `revert_to_official`
    /// treated its records as the user's own — leaving them in place.
    #[test]
    fn known_role_subdomains_are_ours() {
        assert_eq!(
            is_relay_endpoint("https://wb.321ai.cc/v1"),
            Some(RelayHost::Legacy)
        );
        assert_eq!(
            is_relay_endpoint("https://sub2.321ai.cc"),
            Some(RelayHost::Legacy)
        );
        // An unknown label on the same domain is not automatically ours.
        assert_eq!(
            is_relay_endpoint("https://random.321ai.cc"),
            None,
            "only the documented role subdomains are accepted"
        );
    }

    /// Exact names win over subdomain rules, so `api.aimyria.com` stays Gateway.
    #[test]
    fn exact_hosts_are_classified_before_subdomains() {
        assert_eq!(
            is_relay_endpoint("https://api.aimyria.com/v1"),
            Some(RelayHost::Gateway)
        );
        assert_eq!(
            is_relay_endpoint("https://sub2.aimyria.com/v1"),
            Some(RelayHost::Sub2)
        );
    }

    #[test]
    fn bare_hosts_and_env_fragments_classify() {
        assert_eq!(is_relay_endpoint("api.aimyria.com"), Some(RelayHost::Gateway));
        assert_eq!(is_relay_host("AIMYRIA.COM"), Some(RelayHost::Console));
        assert_eq!(
            is_relay_host("api.aimyria.com."),
            Some(RelayHost::Gateway),
            "a trailing dot is a valid FQDN spelling"
        );

        // An `.env` line hands the whole `KEY=value` fragment around. The matcher
        // only classifies endpoints, so a caller that passes a raw line gets the
        // honest answer (not an endpoint) rather than a lucky substring match.
        // Callers that hold config text use the text-level check instead.
        assert_eq!(
            is_relay_endpoint("ANTHROPIC_BASE_URL=https://aimyria.com/v1"),
            None,
            "an env line is not an endpoint"
        );
        // ...but the value out of that line is.
        assert_eq!(
            is_relay_endpoint("https://aimyria.com/v1"),
            Some(RelayHost::Console)
        );
    }

    #[test]
    fn ports_and_userinfo_do_not_change_classification() {
        assert_eq!(
            is_relay_endpoint("https://api.aimyria.com:443/v1"),
            Some(RelayHost::Gateway)
        );
        assert_eq!(
            is_relay_endpoint("https://user:pass@api.aimyria.com/v1"),
            Some(RelayHost::Gateway)
        );
    }

    #[test]
    fn every_host_resolves_its_balance_to_the_gateway() {
        let want = "https://api.aimyria.com/v1/user/balance";
        for host in ALL_HOSTS {
            assert_eq!(host.balance_url(), want, "{host} must use the gateway");
        }
    }

    /// A loopback console base is refused unless the acceptance switch is on.
    ///
    /// This is a security property, not a test convenience: accepting
    /// `http://127.0.0.1` in a shipped build means any local process that binds the
    /// port receives the session token. The switch exists so the acceptance suite
    /// can drive the real flow against a mock; a user never sets it.
    #[test]
    fn a_loopback_console_is_refused_by_default() {
        let _guard = TEST_SWITCH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let previous = std::env::var_os(TEST_CLOCK_ENV);
        std::env::remove_var(TEST_CLOCK_ENV);

        for base in [
            "http://127.0.0.1:8799",
            "http://localhost:8799",
            "http://[::1]:8799",
            "http://192.168.1.10:8799",
        ] {
            assert_eq!(
                is_relay_endpoint(base),
                None,
                "{base} must not be accepted as a console host by default"
            );
        }

        match previous {
            Some(value) => std::env::set_var(TEST_CLOCK_ENV, value),
            None => std::env::remove_var(TEST_CLOCK_ENV),
        }
    }

    /// With the switch on, only loopback is admitted — not a LAN address, not a
    /// remote http host. Widening it would make the switch a backdoor.
    #[test]
    fn the_acceptance_switch_admits_loopback_only() {
        let _guard = TEST_SWITCH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let previous = std::env::var_os(TEST_CLOCK_ENV);
        std::env::set_var(TEST_CLOCK_ENV, "1");

        assert_eq!(
            is_relay_endpoint("http://127.0.0.1:8799"),
            Some(RelayHost::Console)
        );
        assert_eq!(
            is_relay_endpoint("http://localhost:8799/"),
            Some(RelayHost::Console)
        );
        assert_eq!(
            is_relay_endpoint("http://192.168.1.10:8799"),
            None,
            "a LAN address must stay refused even with the switch on"
        );
        assert_eq!(
            is_relay_endpoint("http://evil.example"),
            None,
            "the switch must not admit arbitrary http hosts"
        );
        // https on our real hosts is unaffected.
        assert_eq!(
            is_relay_endpoint("https://aimyria.com"),
            Some(RelayHost::Console)
        );

        match previous {
            Some(value) => std::env::set_var(TEST_CLOCK_ENV, value),
            None => std::env::remove_var(TEST_CLOCK_ENV),
        }
    }
}
