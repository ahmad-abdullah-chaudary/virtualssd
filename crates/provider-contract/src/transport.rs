//! Explains a transport failure in terms the user can act on.
//!
//! `reqwest`'s own message — "error sending request for url ..." — hides the
//! cause. For a network that filters or blocks a host, the cause *is* the
//! answer: the request never reached the server. This module turns the error
//! chain into a sentence that says what happened and what to try.

use std::error::Error;

/// The transport failure, described for a human.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportFailure {
    /// A short description of what went wrong.
    pub summary: String,
    /// The underlying technical detail, when there is one worth showing.
    pub detail: Option<String>,
    /// True when the failure looks like the network refusing the
    /// destination rather than the destination refusing the request.
    pub blocked: bool,
}

impl TransportFailure {
    /// The full message to show the user.
    pub fn message(&self) -> String {
        match &self.detail {
            Some(detail) => format!("{} ({detail})", self.summary),
            None => self.summary.clone(),
        }
    }
}

/// Classifies a `reqwest` transport failure.
pub fn classify(error: &reqwest::Error) -> TransportFailure {
    // Walk the source chain to the root cause. The last entry is the most
    // specific one; the intermediate frames are usually "error sending
    // request for url ...".
    let mut chain: Vec<String> = Vec::new();
    let mut current: Option<&(dyn Error + 'static)> = error.source();
    while let Some(inner) = current {
        let text = inner.to_string();
        if !chain.iter().any(|existing| existing == &text) {
            chain.push(text);
        }
        current = inner.source();
    }
    let root = chain.last().cloned().unwrap_or_else(|| error.to_string());
    let combined = chain.join(" | ").to_ascii_lowercase();

    if error.is_timeout() || combined.contains("timed out") || combined.contains("timeout") {
        return TransportFailure {
            summary: "The request timed out before the server answered".to_string(),
            detail: Some(root),
            blocked: false,
        };
    }

    if combined.contains("dns error") || combined.contains("failed to lookup address") {
        return TransportFailure {
            summary: "The host name could not be resolved".to_string(),
            detail: Some(root),
            blocked: false,
        };
    }

    if combined.contains("certificate")
        || combined.contains("tls")
        || combined.contains("handshake")
    {
        return TransportFailure {
            summary: "The TLS handshake failed".to_string(),
            detail: Some(root),
            blocked: false,
        };
    }

    // A closed or reset connection shows up in several shapes depending on
    // where it was cut: during the send ("connection reset by peer"), during
    // the response headers ("connection closed before message completed"), or
    // as a broken pipe. All of them mean the peer or something on the path
    // dropped the connection, which is how a blocked destination behaves.
    let closed_markers = [
        "connection reset",
        "forcibly closed",
        "connection closed before message completed",
        "incomplete message",
        "broken pipe",
        "connection aborted",
        "eof",
    ];
    if closed_markers
        .iter()
        .any(|marker| combined.contains(marker))
    {
        return TransportFailure {
            summary: "The connection was closed or reset before any data was exchanged. \
                 The network accepted the connection and then dropped it, which is how \
                 a filtered or blocked destination behaves"
                .to_string(),
            detail: Some(root),
            blocked: true,
        };
    }

    if combined.contains("connection refused") {
        return TransportFailure {
            summary: "The connection was refused by the destination".to_string(),
            detail: Some(root),
            blocked: false,
        };
    }

    if combined.contains("network is unreachable") || combined.contains("no route to host") {
        return TransportFailure {
            summary: "There is no route to the destination from this machine".to_string(),
            detail: Some(root),
            blocked: true,
        };
    }

    if error.is_connect() {
        return TransportFailure {
            summary: "The connection to the destination could not be established".to_string(),
            detail: Some(root),
            blocked: true,
        };
    }

    TransportFailure {
        summary: "The request could not be completed".to_string(),
        detail: Some(root),
        blocked: false,
    }
}

/// The advice that follows a transport failure.
pub fn advice(host: &str, failure: &TransportFailure) -> String {
    if failure.blocked {
        return format!(
            "The request to {host} never reached the server. This usually means the \
             network, router or an upstream filter is blocking {host}. Try a different \
             network or a VPN, or set HTTPS_PROXY (or ALL_PROXY for SOCKS5) to a proxy \
             that can reach it."
        );
    }

    match failure.summary.as_str() {
        s if s.contains("timed out") => {
            format!("The request to {host} timed out. Check the connection and try again.")
        }
        s if s.contains("resolved") => format!(
            "The name for {host} could not be resolved. Check the DNS settings and \
             the machine's internet access."
        ),
        s if s.contains("TLS") => format!(
            "The secure connection to {host} could not be established. This is usually \
             caused by a TLS-inspecting firewall or an incorrect system clock."
        ),
        _ => format!("The request to {host} could not be completed."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_plain_request_error_is_described_without_inventing_a_cause() {
        let error = reqwest::Client::new()
            .get("http://127.0.0.1:1/never")
            .timeout(std::time::Duration::from_millis(200))
            .send()
            .await
            .unwrap_err();
        let failure = classify(&error);
        assert!(!failure.summary.is_empty());
        assert!(failure.message().contains(&failure.summary));
    }

    #[tokio::test]
    async fn a_reset_connection_is_reported_as_a_blocked_destination() {
        // A listener that accepts the connection and immediately closes it
        // reproduces what a filtering network does to a blocked host.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();

        std::thread::spawn(move || {
            if let Ok((stream, _)) = listener.accept() {
                drop(stream);
            }
        });

        let error = reqwest::Client::new()
            .get(format!("http://{address}/"))
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await
            .unwrap_err();

        let failure = classify(&error);
        assert!(
            failure.blocked,
            "a reset connection must be reported as a blocked destination, got: {}",
            failure.message()
        );
        assert!(
            failure.message().contains("closed") || failure.message().contains("reset"),
            "the message must mention the closed connection, got: {}",
            failure.message()
        );

        let advice = advice("api.telegram.org", &failure);
        assert!(advice.contains("HTTPS_PROXY"));
    }

    #[test]
    fn advice_names_the_host() {
        let failure = TransportFailure {
            summary: "The connection was reset before any data was exchanged.".to_string(),
            detail: None,
            blocked: true,
        };
        let text = advice("api.telegram.org", &failure);
        assert!(text.contains("api.telegram.org"));
        assert!(text.contains("HTTPS_PROXY"));
    }
}
