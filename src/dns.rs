//! DNS wire format and the transports dnsrace can measure: UDP, TCP, DNS-over-TLS and
//! DNS-over-HTTPS.
//!
//! Everything here is deliberately small: dnsrace builds its own queries and parses only the
//! records it needs. That keeps the desktop app self-contained and makes the measurements easy
//! to reason about.

use std::io::Read;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use tokio::net::TcpStream;
use tokio::net::UdpSocket;
use tokio::time::timeout;

use crate::util::rand64;

pub const TYPE_A: u16 = 1;
pub const TYPE_TXT: u16 = 16;
pub const TYPE_AAAA: u16 = 28;
pub const CLASS_IN: u16 = 1;
pub const CLASS_CH: u16 = 3;

/// Builds a standard query (recursion desired) for one name. Used by the tests; the program
/// itself always goes through [`build_query_ext`].
#[cfg(test)]
pub fn build_query(id: u16, name: &str, qtype: u16, qclass: u16) -> Vec<u8> {
    build_query_ext(id, name, qtype, qclass, false)
}

/// Builds a query. With `dnssec`, it asks for DNSSEC records (the DO bit in an EDNS0 OPT
/// record) so the reply's Authenticated Data bit tells us whether the server validates.
pub fn build_query_ext(id: u16, name: &str, qtype: u16, qclass: u16, dnssec: bool) -> Vec<u8> {
    let mut p = Vec::with_capacity(64);
    p.extend_from_slice(&id.to_be_bytes());
    p.extend_from_slice(&0x0100u16.to_be_bytes()); // flags: recursion desired
    p.extend_from_slice(&1u16.to_be_bytes()); // one question
    p.extend_from_slice(&0u16.to_be_bytes()); // no answers
    p.extend_from_slice(&0u16.to_be_bytes()); // no authority records
    p.extend_from_slice(&(u16::from(dnssec)).to_be_bytes()); // additional: OPT when DNSSEC
    for label in name.trim_end_matches('.').split('.') {
        let l = &label.as_bytes()[..label.len().min(63)];
        p.push(l.len() as u8);
        p.extend_from_slice(l);
    }
    p.push(0);
    p.extend_from_slice(&qtype.to_be_bytes());
    p.extend_from_slice(&qclass.to_be_bytes());
    if dnssec {
        p.push(0); // OPT record: root name
        p.extend_from_slice(&41u16.to_be_bytes()); // type OPT
        p.extend_from_slice(&1232u16.to_be_bytes()); // UDP payload size
        p.extend_from_slice(&[0, 0, 0x80, 0]); // extended rcode/version/flags; 0x8000 = DO
        p.extend_from_slice(&0u16.to_be_bytes()); // no option data
    }
    p
}

/// Skips over a (possibly compressed) domain name and returns the position after it.
fn skip_name(buf: &[u8], mut pos: usize) -> Option<usize> {
    loop {
        let len = *buf.get(pos)? as usize;
        if len == 0 {
            return Some(pos + 1);
        }
        if len & 0xC0 == 0xC0 {
            return Some(pos + 2);
        }
        pos += 1 + len;
    }
}

/// What we need from a reply.
#[derive(Debug, Clone)]
pub struct Parsed {
    pub rcode: u8,
    pub answers: usize,
    pub addrs: Vec<IpAddr>,
    pub txt: Option<String>,
    /// The Authenticated Data bit: the server says it validated the answer with DNSSEC.
    pub ad: bool,
    /// The reply was truncated and should be retried over TCP.
    pub tc: bool,
}

/// Parses a reply. Returns `None` for anything that is not a well-formed response with the
/// expected transaction id.
pub fn parse_reply(buf: &[u8], id: u16) -> Option<Parsed> {
    if buf.len() < 12 || u16::from_be_bytes([buf[0], buf[1]]) != id {
        return None;
    }
    let flags = u16::from_be_bytes([buf[2], buf[3]]);
    if flags & 0x8000 == 0 {
        return None; // not a response
    }
    let questions = u16::from_be_bytes([buf[4], buf[5]]) as usize;
    let answers = u16::from_be_bytes([buf[6], buf[7]]) as usize;
    let mut pos = 12;
    for _ in 0..questions {
        pos = skip_name(buf, pos)? + 4;
    }
    let mut addrs = Vec::new();
    let mut txt = None;
    for _ in 0..answers {
        pos = skip_name(buf, pos)?;
        if pos + 10 > buf.len() {
            break;
        }
        let rtype = u16::from_be_bytes([buf[pos], buf[pos + 1]]);
        let rdlen = u16::from_be_bytes([buf[pos + 8], buf[pos + 9]]) as usize;
        pos += 10;
        if pos + rdlen > buf.len() {
            break;
        }
        match rtype {
            TYPE_A if rdlen == 4 => addrs.push(IpAddr::V4(Ipv4Addr::new(
                buf[pos],
                buf[pos + 1],
                buf[pos + 2],
                buf[pos + 3],
            ))),
            TYPE_AAAA if rdlen == 16 => {
                let mut octets = [0u8; 16];
                octets.copy_from_slice(&buf[pos..pos + 16]);
                addrs.push(IpAddr::V6(Ipv6Addr::from(octets)));
            }
            TYPE_TXT if rdlen >= 1 && txt.is_none() => {
                let len = buf[pos] as usize;
                if len < rdlen {
                    txt = Some(String::from_utf8_lossy(&buf[pos + 1..pos + 1 + len]).into_owned());
                }
            }
            _ => {}
        }
        pos += rdlen;
    }
    Some(Parsed {
        rcode: (flags & 0x000F) as u8,
        answers,
        addrs,
        txt,
        ad: flags & 0x0020 != 0,
        tc: flags & 0x0200 != 0,
    })
}

/// A reply together with how long it took.
#[derive(Debug, Clone)]
pub struct Reply {
    pub rtt: Duration,
    pub rcode: u8,
    pub answers: usize,
    pub addrs: Vec<IpAddr>,
    pub txt: Option<String>,
    /// The server set the DNSSEC Authenticated Data bit.
    pub ad: bool,
    /// The reply was truncated (UDP only).
    pub tc: bool,
}

impl From<Parsed> for Reply {
    fn from(p: Parsed) -> Self {
        Reply {
            rtt: Duration::ZERO,
            rcode: p.rcode,
            answers: p.answers,
            addrs: p.addrs,
            txt: p.txt,
            ad: p.ad,
            tc: p.tc,
        }
    }
}

fn local_bind(server: SocketAddr) -> SocketAddr {
    if server.is_ipv4() {
        SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0))
    } else {
        SocketAddr::from((Ipv6Addr::UNSPECIFIED, 0))
    }
}

/// One UDP lookup. Returns `None` on timeout or network error. The round-trip time runs from
/// just before the send to the arrival of the matching reply.
pub async fn query(
    server: SocketAddr,
    name: &str,
    qtype: u16,
    qclass: u16,
    wait: Duration,
) -> Option<Reply> {
    query_ext(server, name, qtype, qclass, wait, false).await
}

/// Like [`query`], but with an optional DNSSEC (DO bit) request.
pub async fn query_ext(
    server: SocketAddr,
    name: &str,
    qtype: u16,
    qclass: u16,
    wait: Duration,
    dnssec: bool,
) -> Option<Reply> {
    let sock = UdpSocket::bind(local_bind(server)).await.ok()?;
    sock.connect(server).await.ok()?;
    let id = (rand64() & 0xFFFF) as u16;
    let packet = build_query_ext(id, name, qtype, qclass, dnssec);
    let start = Instant::now();
    sock.send(&packet).await.ok()?;
    let deadline = start + wait;
    let mut buf = [0u8; 4096];
    loop {
        let left = deadline.checked_duration_since(Instant::now())?;
        match timeout(left, sock.recv(&mut buf)).await {
            Ok(Ok(n)) => {
                let rtt = start.elapsed();
                if let Some(p) = parse_reply(&buf[..n], id) {
                    let mut reply: Reply = p.into();
                    reply.rtt = rtt;
                    return Some(reply);
                }
                // a stray or mismatched packet: keep waiting for the real reply
            }
            _ => return None,
        }
    }
}

/// DNS-over-HTTPS client. Shared so TLS connections and the HTTP connection pool are reused
/// across measurements.
fn http_agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::AgentBuilder::new()
            .redirects(0)
            .user_agent(concat!("dnsrace/", env!("CARGO_PKG_VERSION")))
            .build()
    })
}

/// One DNS-over-HTTPS lookup using RFC 8484 wire format (GET + `application/dns-message`).
///
/// Returns `None` for HTTP errors, invalid responses and timeouts. The request runs on a
/// blocking thread because `ureq` is synchronous; this keeps it out of the async workers.
pub async fn doh_query(url: &str, name: &str, wait: Duration) -> Option<Reply> {
    let url = url.to_string();
    let name = name.to_string();
    tokio::task::spawn_blocking(move || doh_query_blocking(&url, &name, wait))
        .await
        .ok()?
}

fn doh_query_blocking(url: &str, name: &str, wait: Duration) -> Option<Reply> {
    // RFC 8484 recommends a message ID of 0 for cache friendliness.
    let packet = build_query_ext(0, name, TYPE_A, CLASS_IN, true);
    let encoded = base64url_encode(&packet);
    let separator = if url.contains('?') { '&' } else { '?' };
    let request_url = format!("{url}{separator}dns={encoded}");
    let start = Instant::now();
    let response = http_agent()
        .get(&request_url)
        .set("Accept", "application/dns-message")
        .timeout(wait)
        .call()
        .ok()?;
    if let Some(content_type) = response.header("content-type")
        && !content_type
            .to_ascii_lowercase()
            .starts_with("application/dns-message")
    {
        return None;
    }
    let mut body = Vec::with_capacity(512);
    response.into_reader().read_to_end(&mut body).ok()?;
    let rtt = start.elapsed();
    let mut reply: Reply = parse_reply(&body, 0)?.into();
    reply.rtt = rtt;
    Some(reply)
}

/// Encodes bytes as base64url without padding, as required by RFC 8484.
fn base64url_encode(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    let (chunks, rem) = input.as_chunks::<3>();
    for &chunk in chunks {
        let b0 = chunk[0];
        let b1 = chunk[1];
        let b2 = chunk[2];
        out.push(ALPHABET[(b0 >> 2) as usize] as char);
        out.push(ALPHABET[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        out.push(ALPHABET[(((b1 & 0x0F) << 2) | (b2 >> 6)) as usize] as char);
        out.push(ALPHABET[(b2 & 0x3F) as usize] as char);
    }
    match rem {
        [b0] => {
            out.push(ALPHABET[(b0 >> 2) as usize] as char);
            out.push(ALPHABET[((b0 & 0x03) << 4) as usize] as char);
        }
        [b0, b1] => {
            out.push(ALPHABET[(b0 >> 2) as usize] as char);
            out.push(ALPHABET[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
            out.push(ALPHABET[((b1 & 0x0F) << 2) as usize] as char);
        }
        _ => {}
    }
    out
}

/// Queries a server over TCP instead of UDP. Returns the reply and round-trip time.
pub async fn query_tcp(
    server: SocketAddr,
    name: &str,
    qtype: u16,
    qclass: u16,
    wait: Duration,
) -> Option<Reply> {
    query_tcp_ext(server, name, qtype, qclass, wait, false).await
}

/// Like [`query_tcp`], but with an optional DNSSEC (DO bit) request.
pub async fn query_tcp_ext(
    server: SocketAddr,
    name: &str,
    qtype: u16,
    qclass: u16,
    wait: Duration,
    dnssec: bool,
) -> Option<Reply> {
    let deadline = Instant::now() + wait;
    let stream = timeout(wait, TcpStream::connect(server)).await.ok()?.ok()?;
    let id = (rand64() & 0xFFFF) as u16;
    let packet = build_query_ext(id, name, qtype, qclass, dnssec);
    // TCP DNS prefixes each message with a 2-byte big-endian length.
    let mut framed = Vec::with_capacity(packet.len() + 2);
    framed.extend_from_slice(&(packet.len() as u16).to_be_bytes());
    framed.extend_from_slice(&packet);

    let left = deadline.checked_duration_since(Instant::now())?;
    let start = Instant::now();
    let exchange = async {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let (mut rd, mut wr) = stream.into_split();
        wr.write_all(&framed).await.ok()?;
        let _ = wr.shutdown().await;
        let mut len_buf = [0u8; 2];
        rd.read_exact(&mut len_buf).await.ok()?;
        let msg_len = u16::from_be_bytes(len_buf) as usize;
        let mut buf = vec![0u8; msg_len];
        rd.read_exact(&mut buf).await.ok()?;
        Some(buf)
    };
    let buf = timeout(left, exchange).await.ok()??;
    let rtt = start.elapsed();
    let mut reply: Reply = parse_reply(&buf, id)?.into();
    reply.rtt = rtt;
    Some(reply)
}

// ---------------------------------------------------------------------------------------
// DNS-over-TLS
// ---------------------------------------------------------------------------------------

fn tls_config() -> Arc<rustls::ClientConfig> {
    static CONFIG: OnceLock<Arc<rustls::ClientConfig>> = OnceLock::new();
    CONFIG
        .get_or_init(|| {
            let mut roots = rustls::RootCertStore::empty();
            if let Ok(certs) = rustls_native_certs::load_native_certs() {
                for cert in certs {
                    let _ = roots.add(cert);
                }
            }
            if roots.is_empty() {
                roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            }
            Arc::new(
                rustls::ClientConfig::builder()
                    .with_root_certificates(roots)
                    .with_no_client_auth(),
            )
        })
        .clone()
}

/// One DNS-over-TLS lookup on port 853 (or `port`). `server` may be omitted for a custom
/// hostname, in which case it is resolved with the system resolver first.
///
/// The measured round-trip time starts after the TLS handshake, so it measures the DNS query
/// rather than the certificate exchange.
#[allow(clippy::too_many_arguments)]
pub async fn query_dot(
    server: Option<SocketAddr>,
    host: &str,
    port: u16,
    name: &str,
    qtype: u16,
    qclass: u16,
    wait: Duration,
    dnssec: bool,
) -> Option<Reply> {
    let deadline = Instant::now() + wait;
    let addr = match server {
        Some(addr) => addr,
        None => {
            let lookup = format!("{host}:{port}");
            timeout(wait, tokio::net::lookup_host(lookup))
                .await
                .ok()?
                .ok()?
                .next()?
        }
    };
    let tcp = timeout(wait, TcpStream::connect(addr)).await.ok()?.ok()?;
    let server_name = rustls::pki_types::ServerName::try_from(host.to_string()).ok()?;
    let connector = tokio_rustls::TlsConnector::from(tls_config());
    let left = deadline.checked_duration_since(Instant::now())?;
    let mut stream = timeout(left, connector.connect(server_name, tcp))
        .await
        .ok()?
        .ok()?;

    let id = (rand64() & 0xFFFF) as u16;
    let packet = build_query_ext(id, name, qtype, qclass, dnssec);
    let mut framed = Vec::with_capacity(packet.len() + 2);
    framed.extend_from_slice(&(packet.len() as u16).to_be_bytes());
    framed.extend_from_slice(&packet);

    let left = deadline.checked_duration_since(Instant::now())?;
    let start = Instant::now();
    let exchange = async {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        stream.write_all(&framed).await.ok()?;
        let mut len_buf = [0u8; 2];
        stream.read_exact(&mut len_buf).await.ok()?;
        let msg_len = u16::from_be_bytes(len_buf) as usize;
        let mut buf = vec![0u8; msg_len];
        stream.read_exact(&mut buf).await.ok()?;
        Some(buf)
    };
    let buf = timeout(left, exchange).await.ok()??;
    let rtt = start.elapsed();
    let mut reply: Reply = parse_reply(&buf, id)?.into();
    reply.rtt = rtt;
    Some(reply)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[test]
    fn builds_a_tls_configuration() {
        // Exactly one rustls crypto provider must be enabled. With both aws-lc and ring enabled,
        // ClientConfig::builder() panics at the first DoT connection.
        let config = tls_config();
        assert!(!config.alpn_protocols.iter().any(|p| p.is_empty()));
    }

    #[test]
    fn builds_a_valid_query() {
        let q = build_query(0x1234, "example.com", TYPE_A, CLASS_IN);
        assert_eq!(&q[0..2], &[0x12, 0x34]); // id
        assert_eq!(&q[2..4], &[0x01, 0x00]); // recursion desired
        assert_eq!(&q[4..6], &[0x00, 0x01]); // one question
        assert_eq!(&q[12..], b"\x07example\x03com\x00\x00\x01\x00\x01");
    }

    #[test]
    fn a_dnssec_query_carries_an_opt_record_with_the_do_bit() {
        let q = build_query_ext(7, "example.com", TYPE_A, CLASS_IN, true);
        assert_eq!(&q[10..12], &[0x00, 0x01]); // one additional record
        let opt = &q[q.len() - 11..];
        assert_eq!(opt[0], 0); // root name
        assert_eq!(&opt[1..3], &[0, 41]); // OPT
        assert_eq!(&opt[3..5], &[0x04, 0xD0]); // 1232-byte payload
        assert_eq!(&opt[5..9], &[0, 0, 0x80, 0]); // DO bit
        assert_eq!(&opt[9..11], &[0, 0]); // no options
        // without DNSSEC the additional count stays zero
        assert_eq!(
            &build_query(7, "example.com", TYPE_A, CLASS_IN)[10..12],
            &[0, 0]
        );
    }

    #[test]
    fn a_trailing_dot_and_overlong_labels_are_handled() {
        assert_eq!(
            build_query(1, "example.com.", TYPE_A, CLASS_IN),
            build_query(1, "example.com", TYPE_A, CLASS_IN)
        );
        let long = "a".repeat(100);
        let q = build_query(1, &format!("{long}.com"), TYPE_A, CLASS_IN);
        assert_eq!(q[12], 63); // truncated to the maximum label length
    }

    /// A reply to a query for example.com with the given answer records appended.
    fn reply_with(id: u16, qtype: u16, qclass: u16, rcode: u8, records: &[Vec<u8>]) -> Vec<u8> {
        let mut r = build_query(id, "example.com", qtype, qclass);
        r[2] = 0x81; // QR + RD
        r[3] = 0x80 | rcode; // RA + rcode
        r[7] = records.len() as u8;
        for rec in records {
            r.extend_from_slice(rec);
        }
        r
    }

    /// An answer record whose name is a compression pointer to the question.
    fn record(rtype: u16, rclass: u16, rdata: &[u8]) -> Vec<u8> {
        let mut v = vec![0xC0, 0x0C];
        v.extend_from_slice(&rtype.to_be_bytes());
        v.extend_from_slice(&rclass.to_be_bytes());
        v.extend_from_slice(&[0, 0, 0, 60]); // ttl
        v.extend_from_slice(&(rdata.len() as u16).to_be_bytes());
        v.extend_from_slice(rdata);
        v
    }

    #[test]
    fn parses_a_and_aaaa_records() {
        let v6 = Ipv6Addr::new(0x2606, 0x4700, 0x4700, 0, 0, 0, 0, 0x1111);
        let r = reply_with(
            0xBEEF,
            TYPE_A,
            CLASS_IN,
            0,
            &[
                record(TYPE_A, CLASS_IN, &[93, 184, 216, 34]),
                record(TYPE_AAAA, CLASS_IN, &v6.octets()),
            ],
        );
        let p = parse_reply(&r, 0xBEEF).expect("valid reply");
        assert_eq!(p.rcode, 0);
        assert_eq!(p.answers, 2);
        assert_eq!(
            p.addrs,
            vec![IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34)), IpAddr::V6(v6)]
        );
    }

    #[test]
    fn rejects_wrong_id_non_responses_and_garbage() {
        let r = reply_with(1, TYPE_A, CLASS_IN, 0, &[]);
        assert!(parse_reply(&r, 2).is_none());
        assert!(parse_reply(&[1, 2, 3], 1).is_none());
        let mut query_not_response = r.clone();
        query_not_response[2] = 0x01; // QR bit cleared
        assert!(parse_reply(&query_not_response, 1).is_none());
    }

    #[test]
    fn survives_truncated_replies() {
        let r = reply_with(
            3,
            TYPE_A,
            CLASS_IN,
            0,
            &[record(TYPE_A, CLASS_IN, &[1, 2, 3, 4])],
        );
        for cut in 0..r.len() {
            // must never panic, whatever the cut point
            let _ = parse_reply(&r[..cut], 3);
        }
    }

    #[test]
    fn reads_response_codes_and_the_truncation_bit() {
        let r = reply_with(9, TYPE_A, CLASS_IN, 3, &[]);
        let p = parse_reply(&r, 9).expect("valid");
        assert_eq!(p.rcode, 3);
        assert!(!p.tc);
        let mut tc = r;
        tc[2] |= 0x02;
        assert!(parse_reply(&tc, 9).expect("valid").tc);
    }

    #[test]
    fn reads_the_authenticated_data_bit() {
        let mut r = reply_with(4, TYPE_A, CLASS_IN, 0, &[]);
        assert!(!parse_reply(&r, 4).expect("valid").ad);
        r[3] |= 0x20; // AD
        assert!(parse_reply(&r, 4).expect("valid").ad);
    }

    #[test]
    fn reads_a_chaos_txt_reply() {
        let r = reply_with(
            5,
            TYPE_TXT,
            CLASS_CH,
            0,
            &[record(
                TYPE_TXT,
                CLASS_CH,
                &[5, b'm', b'r', b's', b'0', b'4'],
            )],
        );
        assert_eq!(
            parse_reply(&r, 5).expect("valid").txt.as_deref(),
            Some("mrs04")
        );
    }

    #[test]
    fn base64url_has_no_padding() {
        assert_eq!(base64url_encode(b"f"), "Zg");
        assert_eq!(base64url_encode(b"fo"), "Zm8");
        assert_eq!(base64url_encode(b"foo"), "Zm9v");
        assert_eq!(base64url_encode(b"foobar"), "Zm9vYmFy");
        use base64::Engine;
        let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(base64url_encode(&[0, 255, 16, 42, 7]))
            .unwrap();
        assert_eq!(decoded, vec![0, 255, 16, 42, 7]);
    }

    #[tokio::test]
    async fn queries_a_local_mock_server() {
        let server = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = server.local_addr().unwrap();
        tokio::spawn(async move {
            let mut buf = [0u8; 512];
            let (_, peer) = server.recv_from(&mut buf).await.unwrap();
            let id = u16::from_be_bytes([buf[0], buf[1]]);
            let reply = reply_with(
                id,
                TYPE_A,
                CLASS_IN,
                0,
                &[record(TYPE_A, CLASS_IN, &[1, 2, 3, 4])],
            );
            server.send_to(&reply, peer).await.unwrap();
        });
        let r = query(
            addr,
            "example.com",
            TYPE_A,
            CLASS_IN,
            Duration::from_secs(2),
        )
        .await
        .expect("a reply");
        assert_eq!(r.rcode, 0);
        assert_eq!(r.addrs, vec![IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4))]);
        assert!(r.rtt < Duration::from_secs(1));
    }

    #[tokio::test]
    async fn ignores_a_reply_with_the_wrong_id_and_then_times_out() {
        let server = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = server.local_addr().unwrap();
        tokio::spawn(async move {
            let mut buf = [0u8; 512];
            let (_, peer) = server.recv_from(&mut buf).await.unwrap();
            let wrong_id = u16::from_be_bytes([buf[0], buf[1]]).wrapping_add(1);
            let reply = reply_with(wrong_id, TYPE_A, CLASS_IN, 0, &[]);
            server.send_to(&reply, peer).await.unwrap();
        });
        let started = Instant::now();
        let r = query(
            addr,
            "example.com",
            TYPE_A,
            CLASS_IN,
            Duration::from_millis(300),
        )
        .await;
        assert!(r.is_none());
        assert!(started.elapsed() >= Duration::from_millis(250));
    }

    #[tokio::test]
    async fn times_out_on_a_silent_server() {
        let silent = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = silent.local_addr().unwrap();
        let started = Instant::now();
        let r = query(
            addr,
            "example.com",
            TYPE_A,
            CLASS_IN,
            Duration::from_millis(200),
        )
        .await;
        assert!(r.is_none());
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[tokio::test]
    async fn tcp_reads_are_bounded_by_the_timeout() {
        // Accept the connection, read the query, then never answer. The old implementation
        // waited forever here.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (_stream, _) = listener.accept().await.unwrap();
            tokio::time::sleep(Duration::from_secs(30)).await;
        });
        let started = Instant::now();
        let r = query_tcp(
            addr,
            "example.com",
            TYPE_A,
            CLASS_IN,
            Duration::from_millis(250),
        )
        .await;
        assert!(r.is_none());
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[tokio::test]
    async fn queries_a_local_doh_server_with_wire_format() {
        use base64::Engine;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buf = [0u8; 1024];
            loop {
                let n = stream.read(&mut buf).await.unwrap();
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&buf[..n]);
                if request.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let text = String::from_utf8_lossy(&request);
            let path = text
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .expect("request path");
            assert!(path.contains("dns="));
            assert!(text.contains("Accept: application/dns-message"));
            let encoded = path.split("dns=").nth(1).unwrap();
            let query = base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(encoded)
                .unwrap();
            let id = u16::from_be_bytes([query[0], query[1]]);
            assert_eq!(id, 0);
            let reply = reply_with(
                id,
                TYPE_A,
                CLASS_IN,
                0,
                &[record(TYPE_A, CLASS_IN, &[9, 9, 9, 9])],
            );
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/dns-message\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                reply.len()
            );
            stream.write_all(header.as_bytes()).await.unwrap();
            stream.write_all(&reply).await.unwrap();
        });

        let url = format!("http://{addr}/dns-query");
        let r = doh_query(&url, "example.com", Duration::from_secs(2))
            .await
            .expect("a DoH reply");
        assert_eq!(r.addrs, vec![IpAddr::V4(Ipv4Addr::new(9, 9, 9, 9))]);
        task.await.unwrap();
    }

    #[tokio::test]
    async fn doh_rejects_a_non_dns_content_type() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf).await;
            let body = b"<html>not dns</html>";
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(header.as_bytes()).await.unwrap();
            stream.write_all(body).await.unwrap();
        });
        let url = format!("http://{addr}/dns-query");
        assert!(
            doh_query(&url, "example.com", Duration::from_secs(2))
                .await
                .is_none()
        );
    }

    #[tokio::test]
    async fn queries_an_ipv6_server_when_the_machine_supports_it() {
        let Ok(server) = UdpSocket::bind("[::1]:0").await else {
            return; // no IPv6 loopback here: nothing to test
        };
        let addr = server.local_addr().unwrap();
        tokio::spawn(async move {
            let mut buf = [0u8; 512];
            let (_, peer) = server.recv_from(&mut buf).await.unwrap();
            let id = u16::from_be_bytes([buf[0], buf[1]]);
            let reply = reply_with(
                id,
                TYPE_A,
                CLASS_IN,
                0,
                &[record(TYPE_A, CLASS_IN, &[9, 9, 9, 9])],
            );
            server.send_to(&reply, peer).await.unwrap();
        });
        let r = query(
            addr,
            "example.com",
            TYPE_A,
            CLASS_IN,
            Duration::from_secs(2),
        )
        .await
        .expect("a reply over IPv6");
        assert_eq!(r.addrs, vec![IpAddr::V4(Ipv4Addr::new(9, 9, 9, 9))]);
    }
}
