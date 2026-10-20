use std::net::IpAddr;

use local_ip_address::list_afinet_netifas;

pub fn best_local_ip() -> Option<IpAddr> {
    let interfaces = list_afinet_netifas().ok()?;

    let mut candidates = interfaces
        .into_iter()
        .map(|(_, ip)| ip)
        .filter(|&ip| !is_unusable_ip(ip))
        .collect::<Vec<_>>();

    candidates.sort_by_key(|&ip| (ip_priority(ip), ip_family_priority(ip)));

    candidates.into_iter().next()
}

fn is_unusable_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            ip.is_unspecified() || ip.is_loopback() || ip.is_link_local() || ip.is_multicast() || ip.is_broadcast()
        }
        IpAddr::V6(ip) => ip.is_unspecified() || ip.is_loopback() || ip.is_unicast_link_local() || ip.is_multicast(),
    }
}

fn ip_priority(ip: IpAddr) -> usize {
    match ip {
        IpAddr::V4(ip) if ip.is_private() => match ip.octets()[0] {
            192 => 0,
            172 => 1,
            10 => 2,
            _ => unreachable!(),
        },
        IpAddr::V4(_) => 3,
        IpAddr::V6(_) => 4,
    }
}

fn ip_family_priority(ip: IpAddr) -> usize {
    match ip {
        IpAddr::V4(_) => 0,
        IpAddr::V6(_) => 1,
    }
}
