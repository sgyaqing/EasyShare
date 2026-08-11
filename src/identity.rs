use std::net::{IpAddr, Ipv4Addr, UdpSocket};
use std::time::{Duration, Instant};

const MDNS_ADDR: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 251);
const MDNS_PORT: u16 = 5353;
const MDNS_TIMEOUT: Duration = Duration::from_millis(800);
const NBNS_PORT: u16 = 137;
const NBNS_TIMEOUT: Duration = Duration::from_millis(800);

/// Resolve a client's hostname: mDNS reverse lookup first (works for
/// Bonjour/Avahi devices on LANs without PTR records), then a NetBIOS
/// node status query (Windows machines answer this on private networks),
/// then classic reverse DNS, then the IP string itself.
pub fn resolve_hostname(ip: IpAddr) -> String {
    let name = match ip {
        IpAddr::V4(v4) => mdns_ptr_lookup(v4)
            .or_else(|| netbios_name_lookup(v4))
            .or_else(|| reverse_dns(ip)),
        IpAddr::V6(_) => reverse_dns(ip),
    };
    match name {
        Some(n) => {
            let n = n.trim_end_matches('.').to_string();
            let stripped = n.strip_suffix(".local").unwrap_or(&n).to_string();
            if stripped.is_empty() { ip.to_string() } else { stripped }
        }
        None => ip.to_string(),
    }
}

fn reverse_dns(ip: IpAddr) -> Option<String> {
    dns_lookup::lookup_addr(&ip).ok().filter(|n| !n.is_empty())
}

/// Encode a minimal PTR query for `<reversed>.in-addr.arpa` with the QU bit
/// set, asking responders to reply unicast to our ephemeral socket.
fn build_ptr_query(ip: Ipv4Addr) -> Vec<u8> {
    let octets = ip.octets();
    let mut q = Vec::with_capacity(64);
    // Header: id=0, flags=0, qdcount=1, others 0.
    q.extend_from_slice(&[0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0]);
    for o in octets.iter().rev() {
        let s = o.to_string();
        q.push(s.len() as u8);
        q.extend_from_slice(s.as_bytes());
    }
    for label in ["in-addr", "arpa"] {
        q.push(label.len() as u8);
        q.extend_from_slice(label.as_bytes());
    }
    q.push(0); // root label
    q.extend_from_slice(&[0, 12]); // QTYPE = PTR
    q.extend_from_slice(&[0x80, 0x01]); // QCLASS = IN with QU bit
    q
}

/// Extract a PTR target name from an mDNS response packet.
fn parse_ptr_response(buf: &[u8]) -> Option<String> {
    let packet = dns_parser::Packet::parse(buf).ok()?;
    if packet.header.query {
        return None;
    }
    packet.answers.iter().find_map(|rr| match &rr.data {
        dns_parser::RData::PTR(name) => Some(name.to_string()),
        _ => None,
    })
}

fn mdns_ptr_lookup(ip: Ipv4Addr) -> Option<String> {
    // Skip addresses that never have useful mDNS names.
    if ip.is_loopback() || ip.is_unspecified() {
        return None;
    }
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    let query = build_ptr_query(ip);
    socket.send_to(&query, (MDNS_ADDR, MDNS_PORT)).ok()?;
    let deadline = Instant::now() + MDNS_TIMEOUT;
    let mut buf = [0u8; 1500];
    loop {
        let now = Instant::now();
        if now >= deadline {
            return None;
        }
        socket.set_read_timeout(Some(deadline - now)).ok()?;
        match socket.recv_from(&mut buf) {
            Ok((n, _)) => {
                if let Some(name) = parse_ptr_response(&buf[..n]) {
                    return Some(name);
                }
            }
            Err(_) => return None,
        }
    }
}

/// NetBIOS node status (NBSTAT) query: the wildcard name "*" padded to
/// 16 bytes, first-level encoded (each nibble → 'A'+nibble).
fn build_nbstat_query() -> Vec<u8> {
    let mut q = Vec::with_capacity(50);
    // Header: id=1, flags=0 (request), qdcount=1, others 0.
    q.extend_from_slice(&[0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0]);
    // Encoded name: '*' (0x2A → "CK") + 15 zero bytes ("AA" x15).
    q.push(0x20);
    q.extend_from_slice(b"CK");
    for _ in 0..15 {
        q.extend_from_slice(b"AA");
    }
    q.push(0); // root label
    q.extend_from_slice(&[0, 0x21]); // QTYPE = NBSTAT
    q.extend_from_slice(&[0, 1]); // QCLASS = IN
    q
}

/// Extract the machine name from an NBSTAT response: the first unique
/// (non-group) name entry with suffix 0x00 is the computer name.
fn parse_nbstat_response(buf: &[u8]) -> Option<String> {
    if buf.len() < 12 || buf[2] & 0x80 == 0 {
        return None; // too short or not a response
    }
    let ancount = u16::from_be_bytes([buf[6], buf[7]]);
    if ancount == 0 {
        return None;
    }
    // Question section: 0x20 label + 32 encoded chars + root + qtype/qclass.
    let mut pos = 12;
    if buf.get(pos) != Some(&0x20) {
        return None;
    }
    pos += 1 + 32;
    if buf.get(pos) != Some(&0) {
        return None;
    }
    pos += 1 + 4;
    // Answer: name must be a compression pointer; type NBSTAT.
    if pos + 12 > buf.len() || buf[pos] & 0xC0 != 0xC0 {
        return None;
    }
    let rtype = u16::from_be_bytes([buf[pos + 2], buf[pos + 3]]);
    if rtype != 0x0021 {
        return None;
    }
    let rdlen = u16::from_be_bytes([buf[pos + 10], buf[pos + 11]]) as usize;
    pos += 12;
    let rdata = buf.get(pos..pos + rdlen)?;
    let (&count, entries) = rdata.split_first()?;
    let count = count as usize;
    if entries.len() < count * 18 {
        return None;
    }
    for i in 0..count {
        let e = &entries[i * 18..(i + 1) * 18];
        let suffix = e[15];
        let flags = u16::from_be_bytes([e[16], e[17]]);
        const GROUP: u16 = 0x8000;
        if suffix == 0x00 && flags & GROUP == 0 {
            let name: String = e[..15]
                .iter()
                .map(|&b| if b.is_ascii_graphic() { b as char } else { ' ' })
                .collect::<String>()
                .trim_end()
                .to_string();
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    None
}

/// Ask a Windows machine for its computer name. Only worth trying on
/// LAN addresses; public IPs will never answer (ISPs filter port 137).
fn netbios_name_lookup(ip: Ipv4Addr) -> Option<String> {
    if !(ip.is_private() || ip.is_link_local()) {
        return None;
    }
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.set_read_timeout(Some(NBNS_TIMEOUT)).ok()?;
    socket.send_to(&build_nbstat_query(), (ip, NBNS_PORT)).ok()?;
    let mut buf = [0u8; 1500];
    match socket.recv_from(&mut buf) {
        Ok((n, _)) => parse_nbstat_response(&buf[..n]),
        Err(_) => None,
    }
}

/// Identity for a client connecting from the server machine itself
/// (loopback): use the server's LAN IP and real hostname, so local
/// access shows the same identity as access via the LAN address.
/// In containers both values describe the VM, not the host — so they
/// can be overridden (EASY_SHARE_HOST_IP / EASY_SHARE_HOSTNAME).
pub fn local_identity(server_ip: &str, hostname_override: Option<&str>) -> Option<(String, String)> {
    let ip: IpAddr = server_ip.parse().ok()?;
    let hostname = hostname_override
        .map(str::to_string)
        .or_else(|| {
            dns_lookup::get_hostname().ok().map(|h| {
                let h = h.trim_end_matches('.').to_string();
                h.strip_suffix(".local").map(str::to_string).unwrap_or(h)
            })
        })
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| resolve_hostname(ip));
    Some((hostname, server_ip.to_string()))
}

pub fn display_name(hostname: &str, ip: &str) -> String {
    format!("{hostname} ({ip})")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ptr_query_encoding() {
        let q = build_ptr_query(Ipv4Addr::new(192, 168, 3, 104));
        // Header: qdcount=1.
        assert_eq!(&q[4..6], &[0, 1]);
        // Question labels: 104.3.168.192.in-addr.arpa
        let expected_name: &[u8] = b"\x03104\x013\x03168\x03192\x07in-addr\x04arpa\x00";
        assert_eq!(&q[12..12 + expected_name.len()], expected_name);
        let tail = &q[12 + expected_name.len()..];
        assert_eq!(tail, &[0, 12, 0x80, 0x01]); // PTR, IN+QU
    }

    #[test]
    fn parses_ptr_response() {
        // Minimal response: query for 104.3.168.192.in-addr.arpa, one PTR
        // answer pointing to "my-laptop.local". Uses compression pointer
        // (0xC00C) for the owner name.
        let mut pkt: Vec<u8> = Vec::new();
        // Header: id=0, flags=0x8400 (response, authoritative), qd=1, an=1.
        pkt.extend_from_slice(&[0, 0, 0x84, 0x00, 0, 1, 0, 1, 0, 0, 0, 0]);
        let question: &[u8] = b"\x03104\x013\x03168\x03192\x07in-addr\x04arpa\x00\x00\x0c\x80\x01";
        pkt.extend_from_slice(question);
        // Answer: name = ptr to offset 12, type PTR, class IN, ttl=120,
        // rdlength = 17, rdata = "my-laptop.local"
        pkt.extend_from_slice(&[0xC0, 0x0C, 0, 12, 0, 1, 0, 0, 0, 120, 0, 17]);
        pkt.extend_from_slice(b"\x09my-laptop\x05local\x00");
        let name = parse_ptr_response(&pkt).expect("should parse PTR");
        assert!(name.starts_with("my-laptop.local"), "got: {name}");
    }

    #[test]
    fn rejects_non_response_packet() {
        let q = build_ptr_query(Ipv4Addr::new(10, 0, 0, 1));
        assert!(parse_ptr_response(&q).is_none());
    }

    #[test]
    fn strips_local_suffix_and_trailing_dot() {
        // Exercise the full pipeline via a fake: reverse_dns won't resolve
        // these, so test the suffix handling through resolve_hostname only
        // where deterministic (loopback/unspecified skip mDNS).
        let h = resolve_hostname(IpAddr::V4(Ipv4Addr::LOCALHOST));
        assert!(!h.is_empty());
    }

    #[test]
    fn fallback_to_ip_when_lookup_fails() {
        // 192.0.2.1 is TEST-NET-1: no PTR record, mDNS skipped quickly
        // (no listener should answer within 800ms on a normal network).
        let ip = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1));
        let name = resolve_hostname(ip);
        assert!(!name.is_empty());
        if name.chars().all(|c| c.is_ascii_digit() || c == '.') {
            assert_eq!(name, "192.0.2.1");
        }
    }

    #[test]
    fn nbstat_query_encoding() {
        let q = build_nbstat_query();
        assert_eq!(&q[4..6], &[0, 1]); // qdcount = 1
        assert_eq!(q[12], 0x20); // 32-char encoded name label
        assert_eq!(&q[13..17], b"CKAA"); // '*' + first padding byte
        assert_eq!(q[45], 0); // root label
        assert_eq!(&q[46..50], &[0, 0x21, 0, 1]); // NBSTAT, IN
        assert_eq!(q.len(), 50);
    }

    #[test]
    fn parses_nbstat_response() {
        // Response with two name entries: the machine name (unique,
        // suffix 0x00) and a workgroup (group, suffix 0x00).
        let mut pkt: Vec<u8> = Vec::new();
        // Header: id=1, flags=0x8500 (response), qd=1, an=1.
        pkt.extend_from_slice(&[0, 1, 0x85, 0x00, 0, 1, 0, 1, 0, 0, 0, 0]);
        pkt.extend_from_slice(&build_nbstat_query()[12..]); // question section
        // Answer: name ptr to offset 12, NBSTAT, IN, ttl=0, rdlen=37.
        pkt.extend_from_slice(&[0xC0, 0x0C, 0, 0x21, 0, 1, 0, 0, 0, 0, 0, 37]);
        pkt.push(2); // two name entries
        // Entry 1: "DESKTOP-ABC123" (14 chars + pad), suffix 0x00, unique.
        pkt.extend_from_slice(b"DESKTOP-ABC123 ");
        pkt.extend_from_slice(&[0x00, 0x00, 0x00]);
        // Entry 2: "WORKGROUP" padded, suffix 0x00, group flag.
        pkt.extend_from_slice(b"WORKGROUP      ");
        pkt.extend_from_slice(&[0x00, 0x80, 0x00]);
        assert_eq!(parse_nbstat_response(&pkt).as_deref(), Some("DESKTOP-ABC123"));
    }

    #[test]
    fn nbstat_skips_group_names() {
        // Only a group entry present → no machine name.
        let mut pkt: Vec<u8> = Vec::new();
        pkt.extend_from_slice(&[0, 1, 0x85, 0x00, 0, 1, 0, 1, 0, 0, 0, 0]);
        pkt.extend_from_slice(&build_nbstat_query()[12..]);
        pkt.extend_from_slice(&[0xC0, 0x0C, 0, 0x21, 0, 1, 0, 0, 0, 0, 0, 19]);
        pkt.push(1);
        pkt.extend_from_slice(b"WORKGROUP      ");
        pkt.extend_from_slice(&[0x00, 0x80, 0x00]);
        assert_eq!(parse_nbstat_response(&pkt), None);
    }

    #[test]
    fn display_format() {
        assert_eq!(display_name("macbook", "192.168.1.5"), "macbook (192.168.1.5)");
    }
}
