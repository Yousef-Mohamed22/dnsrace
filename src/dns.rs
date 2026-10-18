//! Just enough of the DNS wire format to time A, AAAA and TXT lookups over UDP.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::{Duration, Instant};

use tokio::net::UdpSocket;
use tokio::time::timeout;

use crate::util::rand64;

pub const TYPE_A: u16 = 1;
pub const TYPE_TXT: u16 = 16;
pub const TYPE_AAAA: u16 = 28;
pub const CLASS_IN: u16 = 1;
pub const CLASS_CH: u16 = 3;

/// Builds a standard query (recursion desired) for one name.
pub fn build_query(id: u16, name: &str, qtype: u16, qclass: u16) -> Vec<u8> {
    let mut p = Vec::with_capacity(64);
    p.extend_from_slice(&id.to_be_bytes());
    p.extend_from_slice(&0x0100u16.to_be_bytes()); // flags: recursion desired
    p.extend_from_slice(&1u16.to_be_bytes()); // one question
    p.extend_from_slice(&[0; 6]); // no answer, authority or additional records
    for label in name.trim_end_matches('.').split('.') {
        let l = &label.as_bytes()[..label.len().min(63)];
        p.push(l.len() as u8);
        p.extend_from_slice(l);
    }
    p.push(0);
    p.extend_from_slice(&qtype.to_be_bytes());
    p.extend_from_slice(&qclass.to_be_bytes());
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
#[derive(Debug)]
pub struct Parsed {
    pub rcode: u8,
    pub answers: usize,
    pub addrs: Vec<IpAddr>,
    pub txt: Option<String>,
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
    })
}

/// A reply together with how long it took.
#[derive(Debug)]
pub struct Reply {
    pub rtt: Duration,
    pub rcode: u8,
    pub answers: usize,
    pub addrs: Vec<IpAddr>,
    pub txt: Option<String>,
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
    let local = if server.is_ipv4() {
        SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0))
    } else {
        SocketAddr::from((Ipv6Addr::UNSPECIFIED, 0))
    };
    let sock = UdpSocket::bind(local).await.ok()?;
    sock.connect(server).await.ok()?;
    let id = (rand64() & 0xFFFF) as u16;
    let packet = build_query(id, name, qtype, qclass);
    let start = Instant::now();
    sock.send(&packet).await.ok()?;
    let deadline = start + wait;
    let mut buf = [0u8; 1500];
    loop {
        let left = deadline.checked_duration_since(Instant::now())?;
        match timeout(left, sock.recv(&mut buf)).await {
            Ok(Ok(n)) => {
                let rtt = start.elapsed();
                if let Some(p) = parse_reply(&buf[..n], id) {
                    return Some(Reply {
                        rtt,
                        rcode: p.rcode,
                        answers: p.answers,
                        addrs: p.addrs,
                        txt: p.txt,
                    });
                }
                // a stray or mismatched packet: keep waiting for the real reply
            }
            _ => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_a_valid_query() {
        let q = build_query(0x1234, "example.com", TYPE_A, CLASS_IN);
        assert_eq!(&q[0..2], &[0x12, 0x34]); // id
        assert_eq!(&q[2..4], &[0x01, 0x00]); // recursion desired
        assert_eq!(&q[4..6], &[0x00, 0x01]); // one question
        assert_eq!(&q[12..], b"\x07example\x03com\x00\x00\x01\x00\x01");
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
    fn reads_response_codes() {
        let r = reply_with(9, TYPE_A, CLASS_IN, 3, &[]);
        assert_eq!(parse_reply(&r, 9).expect("valid").rcode, 3);
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
