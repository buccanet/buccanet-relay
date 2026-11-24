//! The address a request comes from, for limits keyed on it. Behind a reverse proxy every
//! request arrives from the proxy, so the relay reads the header that proxy sets
//! (`CLIENT_IP_HEADER`) and trusts it only because nothing else can reach the relay.

use std::net::{IpAddr, SocketAddr};

use axum::extract::{ConnectInfo, FromRequestParts};
use axum::http::request::Parts;

use crate::error::AppError;
use crate::state::AppState;

/// The client's address, IPv6 grouped by /64 since one host owns a whole /64.
pub struct ClientAddr(pub String);

impl FromRequestParts<AppState> for ClientAddr {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let forwarded = state.client_ip_header.as_ref().and_then(|header| parts.headers.get(header)).and_then(|value| value.to_str().ok());
        let ip = match forwarded {
            Some(value) => value.rsplit(',').next().and_then(|last| last.trim().parse::<IpAddr>().ok()),
            None => parts.extensions.get::<ConnectInfo<SocketAddr>>().map(|ConnectInfo(peer)| peer.ip()),
        };
        let ip = ip.ok_or_else(|| AppError::invalid("The client address is unknown."))?;
        Ok(ClientAddr(group(ip)))
    }
}

fn group(ip: IpAddr) -> String {
    match ip {
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => v4.to_string(),
            None => {
                let segments = v6.segments();
                format!("{:x}:{:x}:{:x}:{:x}::/64", segments[0], segments[1], segments[2], segments[3])
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_ipv6_by_prefix() {
        assert_eq!(group("203.0.113.9".parse().unwrap()), "203.0.113.9");
        assert_eq!(group("2001:db8:1:2:aaaa::1".parse().unwrap()), "2001:db8:1:2::/64");
        assert_eq!(group("::ffff:203.0.113.9".parse().unwrap()), "203.0.113.9");
    }
}
