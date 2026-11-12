//! SSRF guards for page fetching: which addresses count as public, a DNS resolver that
//! drops non-public answers, a redirect policy, and a post-connect peer check.

use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::pin::Pin;
use std::task::{Context, Poll};

use hyper_util::client::legacy::connect::{Connection, HttpInfo};
use url::{Host, Url};

/// Redirect hops [`redirect_allowed`] follows before giving up.
pub const MAX_REDIRECTS: usize = 10;

/// True when `ip` is a globally routable unicast address. False for loopback, RFC 1918
/// private, link-local (incl. the 169.254.169.254 cloud metadata endpoint), CGNAT
/// 100.64/10, multicast, broadcast, unspecified, documentation/benchmark/reserved
/// ranges, IPv6 ULA `fc00::/7` and link-local `fe80::/10`, and the IPv4-mapped,
/// IPv4-compatible, NAT64 and 6to4 forms of any of those.
pub fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_v4(v4),
        IpAddr::V6(v6) => is_public_v6(v6),
    }
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    !(ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_multicast()
        || ip.is_broadcast()
        || ip.is_unspecified()
        || ip.is_documentation()
        || o[0] == 0
        || (o[0] == 100 && (o[1] & 0xc0) == 64)
        || (o[0] == 192 && o[1] == 0 && o[2] == 0)
        || (o[0] == 198 && (o[1] & 0xfe) == 18)
        || o[0] >= 240)
}

fn is_public_v6(ip: Ipv6Addr) -> bool {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return is_public_v4(v4);
    }
    let s = ip.segments();
    let embedded = |hi: u16, lo: u16| Ipv4Addr::from((u32::from(hi) << 16) | u32::from(lo));
    if s[0] == 0x0064 && s[1] == 0xff9b {
        // NAT64 64:ff9b::/96 reaches the embedded IPv4 address.
        return is_public_v4(embedded(s[6], s[7]));
    }
    if s[0] == 0x2002 {
        // 6to4 2002:AABB:CCDD::/48 tunnels to AA.BB.CC.DD.
        return is_public_v4(embedded(s[1], s[2]));
    }
    !(ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || s[..6].iter().all(|x| *x == 0) // ::a.b.c.d (IPv4-compatible, deprecated)
        || (s[0] & 0xfe00) == 0xfc00 // unique local fc00::/7
        || (s[0] & 0xffc0) == 0xfe80 // link-local fe80::/10
        || (s[0] & 0xffc0) == 0xfec0 // site-local fec0::/10 (deprecated)
        || (s[0] == 0x2001 && s[1] == 0x0db8) // documentation
        || (s[0] == 0x2001 && s[1] == 0)) // Teredo
}

/// True when the URL's host is `localhost` (or `*.localhost`) or a literal IP that is
/// not [public](is_public_ip). Hostnames are not resolved here; [`SafeResolver`] does that.
pub fn is_local_url(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(d)) => {
            let d = d.trim_end_matches('.').to_ascii_lowercase();
            d == "localhost" || d.ends_with(".localhost")
        }
        Some(Host::Ipv4(ip)) => !is_public_v4(ip),
        Some(Host::Ipv6(ip)) => !is_public_v6(ip),
        None => true,
    }
}

/// Whether to follow a redirect to `next` after `previous_hops` redirects: http(s) only,
/// not to a [local](is_local_url) host, at most [`MAX_REDIRECTS`] hops.
pub fn redirect_allowed(next: &Url, previous_hops: usize) -> bool {
    previous_hops < MAX_REDIRECTS
        && matches!(next.scheme(), "http" | "https")
        && !is_local_url(next)
}

/// Redirect policy for the safe client; see [`redirect_allowed`].
pub(crate) fn redirect_policy() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        if redirect_allowed(attempt.url(), attempt.previous().len()) {
            attempt.follow()
        } else {
            let msg = format!("refusing redirect to {}", attempt.url());
            attempt.error(msg)
        }
    })
}

/// DNS resolver that resolves with the system resolver (`getaddrinfo` via
/// `tokio::net::lookup_host`) and keeps only [public](is_public_ip) addresses; a name with
/// none left is an error, so the request never connects.
#[derive(Debug, Clone, Copy, Default)]
pub struct SafeResolver;

impl reqwest::dns::Resolve for SafeResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let host = name.as_str().to_string();
        Box::pin(async move {
            let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), 0))
                .await?
                .filter(|a| is_public_ip(a.ip()))
                .collect();
            if addrs.is_empty() {
                return Err(format!("{host} resolves only to non-public addresses").into());
            }
            let addrs: reqwest::dns::Addrs = Box::new(addrs.into_iter());
            Ok(addrs)
        })
    }
}

type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// Connector layer that checks the address each new connection actually reached. Literal
/// IP URLs never pass through the resolver, and this also covers anything else that
/// would bypass it; no request bytes are sent to a non-public peer.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct PublicPeerLayer;

impl<S> tower_layer::Layer<S> for PublicPeerLayer {
    type Service = PublicPeer<S>;
    fn layer(&self, inner: S) -> PublicPeer<S> {
        PublicPeer { inner }
    }
}

/// See [`PublicPeerLayer`].
#[derive(Debug, Clone)]
pub(crate) struct PublicPeer<S> {
    inner: S,
}

impl<S, R> tower_service::Service<R> for PublicPeer<S>
where
    S: tower_service::Service<R, Error = BoxError>,
    S::Response: Connection + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = S::Response;
    type Error = BoxError;
    type Future = Pin<Box<dyn Future<Output = Result<S::Response, BoxError>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), BoxError>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: R) -> Self::Future {
        let connecting = self.inner.call(req);
        Box::pin(async move {
            let conn = connecting.await?;
            let mut extras = http::Extensions::new();
            conn.connected().get_extras(&mut extras);
            match extras.get::<HttpInfo>().map(HttpInfo::remote_addr) {
                Some(peer) if is_public_ip(peer.ip()) => Ok(conn),
                Some(peer) => {
                    Err(format!("refusing to fetch from non-public address {}", peer.ip()).into())
                }
                None => Err("refusing to fetch: peer address unknown".into()),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_public_ip_table() {
        let cases: &[(&str, bool)] = &[
            ("8.8.8.8", true),
            ("1.1.1.1", true),
            ("93.184.216.34", true),
            ("172.32.0.1", true),
            ("100.128.0.1", true),
            ("2606:4700:4700::1111", true),
            ("127.0.0.1", false),
            ("127.255.255.254", false),
            ("10.0.0.1", false),
            ("172.16.0.1", false),
            ("172.31.255.255", false),
            ("192.168.1.1", false),
            ("169.254.169.254", false),
            ("169.254.0.1", false),
            ("100.64.0.1", false),
            ("100.127.255.255", false),
            ("224.0.0.1", false),
            ("239.255.255.250", false),
            ("0.0.0.0", false),
            ("255.255.255.255", false),
            ("::1", false),
            ("::", false),
            ("fc00::1", false),
            ("fd12:3456::1", false),
            ("fe80::1", false),
            ("ff02::1", false),
            ("::ffff:127.0.0.1", false),
            ("::ffff:10.0.0.1", false),
            ("::ffff:169.254.169.254", false),
            ("::ffff:8.8.8.8", true),
        ];
        for (ip, public) in cases {
            let parsed: IpAddr = ip.parse().unwrap();
            assert_eq!(is_public_ip(parsed), *public, "{ip}");
        }
    }

    #[test]
    fn local_hosts_are_detected() {
        for (u, local) in [
            ("http://localhost/x", true),
            ("http://LOCALHOST:8080/x", true),
            ("http://app.localhost/", true),
            ("http://127.0.0.1:9/x", true),
            ("http://[::1]/", true),
            ("http://10.1.2.3/", true),
            ("http://169.254.169.254/latest/meta-data", true),
            ("https://example.com/", false),
            ("https://8.8.8.8/", false),
        ] {
            assert_eq!(is_local_url(&url::Url::parse(u).unwrap()), local, "{u}");
        }
    }

    #[tokio::test]
    async fn safe_resolver_drops_private_answers() {
        use reqwest::dns::Resolve;
        let r = SafeResolver;
        let err = r.resolve("localhost".parse().unwrap()).await;
        assert!(err.is_err(), "localhost resolves only to loopback");
    }
}
