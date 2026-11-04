//! The HTTP client the trackers' calls share.

use std::time::Duration;

/// An HTTP client that trusts what macOS trusts (so a company's own certificates too), gives up
/// after 15 seconds, and hands back error statuses for the caller to explain.
pub fn agent() -> ureq::Agent {
    let tls = ureq::tls::TlsConfig::builder().provider(ureq::tls::TlsProvider::NativeTls).build();
    ureq::Agent::config_builder()
        .tls_config(tls)
        .timeout_global(Some(Duration::from_secs(15)))
        .http_status_as_error(false)
        .build()
        .new_agent()
}
