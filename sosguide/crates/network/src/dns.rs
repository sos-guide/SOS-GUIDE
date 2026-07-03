//! Serveur DNS minimal de **portail captif**, fait main (zéro dépendance).
//!
//! Comportement : **toute** requête `A` reçoit l'IP du nœud (ce qui déclenche la
//! détection de portail captif côté client) ; `sos.guide` n'est donc qu'un cas
//! particulier de cette règle. Les requêtes `AAAA` reçoivent une réponse vide
//! (`NOERROR`, zéro enregistrement) car **l'IPv6 est désactivée**. Aucun autre
//! type n'est servi (réponse vide). Aucun enregistrement n'est jamais relayé
//! vers l'extérieur : le nœud est une île.
//!
//! Le codec (`parse_query`/`build_response`) est pur et entièrement testé ; la
//! boucle réseau (`serve`) est volontairement mince.

use std::net::{Ipv4Addr, SocketAddr};

use tokio::net::UdpSocket;

/// Type d'enregistrement A (IPv4).
const TYPE_A: u16 = 1;
/// Classe Internet.
const CLASS_IN: u16 = 1;
/// TTL court (s) : le portail captif ne doit pas être mis en cache longtemps.
const ANSWER_TTL: u32 = 30;
/// Taille d'un message DNS sur UDP sans EDNS (RFC 1035).
const MAX_UDP: usize = 512;
/// Garde anti-boucle sur le nombre de labels d'un nom.
const MAX_LABELS: usize = 127;

/// Erreur d'analyse d'une requête DNS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DnsError {
    /// Message plus court que l'en-tête, ou tronqué en cours d'analyse.
    #[error("message DNS tronqué")]
    Truncated,
    /// Aucune question dans le message (`QDCOUNT == 0`).
    #[error("message DNS sans question")]
    NoQuestion,
    /// Nom de domaine mal formé (pointeur de compression ou label invalide).
    #[error("nom de domaine DNS mal formé")]
    BadName,
}

/// Requête DNS analysée (première question seulement).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    /// Identifiant de transaction (réémis tel quel dans la réponse).
    pub id: u16,
    /// Drapeaux de l'en-tête (on en réutilise le bit RD).
    pub flags: u16,
    /// Nom demandé, en minuscules et points (ex. `connectivitycheck.gstatic.com`).
    pub name: String,
    /// Type demandé (`A`, `AAAA`, …).
    pub qtype: u16,
    /// Classe demandée (`IN`).
    pub qclass: u16,
    /// Octets bruts de la question (qname + qtype + qclass), réémis tels quels.
    pub question: Vec<u8>,
}

/// Lit un nom DNS à partir de `pos`. Refuse les pointeurs de compression : une
/// question n'en contient jamais. Renvoie le nom et l'offset juste après le
/// `0x00` final.
fn read_name(buf: &[u8], start: usize) -> Result<(String, usize), DnsError> {
    let mut labels: Vec<String> = Vec::new();
    let mut pos = start;
    loop {
        let &len = buf.get(pos).ok_or(DnsError::Truncated)?;
        let len = len as usize;
        if len == 0 {
            pos += 1;
            break;
        }
        if len & 0xC0 != 0 {
            // Pointeur de compression : interdit dans une question.
            return Err(DnsError::BadName);
        }
        if labels.len() >= MAX_LABELS {
            return Err(DnsError::BadName);
        }
        let from = pos + 1;
        let to = from + len;
        let label = buf.get(from..to).ok_or(DnsError::Truncated)?;
        labels.push(String::from_utf8_lossy(label).to_lowercase());
        pos = to;
    }
    Ok((labels.join("."), pos))
}

/// Lit un `u16` big-endian à `pos` sans indexation directe.
fn read_u16(buf: &[u8], pos: usize) -> Result<u16, DnsError> {
    let bytes: [u8; 2] = buf
        .get(pos..pos + 2)
        .ok_or(DnsError::Truncated)?
        .try_into()
        .map_err(|_| DnsError::Truncated)?;
    Ok(u16::from_be_bytes(bytes))
}

/// Analyse la première question d'un message DNS.
pub fn parse_query(buf: &[u8]) -> Result<Query, DnsError> {
    let id = read_u16(buf, 0)?;
    let flags = read_u16(buf, 2)?;
    let qdcount = read_u16(buf, 4)?;
    if qdcount == 0 {
        return Err(DnsError::NoQuestion);
    }
    let (name, after_name) = read_name(buf, 12)?;
    let qtype = read_u16(buf, after_name)?;
    let qclass = read_u16(buf, after_name + 2)?;
    let question = buf
        .get(12..after_name + 4)
        .ok_or(DnsError::Truncated)?
        .to_vec();
    Ok(Query {
        id,
        flags,
        name,
        qtype,
        qclass,
        question,
    })
}

/// Construit la réponse à une requête : `A` → IP du nœud ; tout le reste
/// (`AAAA` compris) → réponse vide `NOERROR`.
#[must_use]
pub fn build_response(query: &Query, node_ip: Ipv4Addr) -> Vec<u8> {
    let answer = query.qtype == TYPE_A && query.qclass == CLASS_IN;
    let mut out = Vec::with_capacity(12 + query.question.len() + 16);

    out.extend_from_slice(&query.id.to_be_bytes());
    // QR=1, AA=1, RD recopié de la requête, RCODE=0.
    let rd = query.flags & 0x0100;
    let resp_flags: u16 = 0x8400 | rd;
    out.extend_from_slice(&resp_flags.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes()); // QDCOUNT
    out.extend_from_slice(&u16::from(answer).to_be_bytes()); // ANCOUNT
    out.extend_from_slice(&0u16.to_be_bytes()); // NSCOUNT
    out.extend_from_slice(&0u16.to_be_bytes()); // ARCOUNT
    out.extend_from_slice(&query.question);

    if answer {
        // Pointeur de compression vers le nom de la question (offset 12).
        out.extend_from_slice(&[0xC0, 0x0C]);
        out.extend_from_slice(&TYPE_A.to_be_bytes());
        out.extend_from_slice(&CLASS_IN.to_be_bytes());
        out.extend_from_slice(&ANSWER_TTL.to_be_bytes());
        out.extend_from_slice(&4u16.to_be_bytes()); // RDLENGTH
        out.extend_from_slice(&node_ip.octets());
    }
    out
}

/// Boucle de service DNS : répond à chaque datagramme jusqu'à erreur de socket.
/// Toute requête mal formée est ignorée (jamais de panique, jamais de fuite).
pub async fn serve(socket: UdpSocket, node_ip: Ipv4Addr) {
    let mut buf = [0u8; MAX_UDP];
    loop {
        let (len, peer) = match socket.recv_from(&mut buf).await {
            Ok(v) => v,
            Err(err) => {
                tracing::warn!(%err, "DNS: réception impossible — arrêt de la boucle");
                return;
            }
        };
        let datagram = match buf.get(..len) {
            Some(d) => d,
            None => continue,
        };
        match parse_query(datagram) {
            Ok(query) => {
                let response = build_response(&query, node_ip);
                if let Err(err) = socket.send_to(&response, peer).await {
                    tracing::warn!(%err, %peer, "DNS: envoi de la réponse impossible");
                }
            }
            Err(err) => tracing::trace!(%err, %peer, "DNS: requête ignorée"),
        }
    }
}

/// Lie un socket UDP DNS sur `addr` (helper pour l'orchestrateur).
pub async fn bind(addr: SocketAddr) -> std::io::Result<UdpSocket> {
    UdpSocket::bind(addr).await
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    /// Type d'enregistrement AAAA (IPv6) — seulement utile pour vérifier la
    /// réponse vide côté tests (l'IPv6 est désactivée).
    const TYPE_AAAA: u16 = 28;

    /// Encode une question DNS : en-tête + `name` + qtype + qclass.
    fn make_query(id: u16, name: &str, qtype: u16) -> Vec<u8> {
        let mut q = Vec::new();
        q.extend_from_slice(&id.to_be_bytes());
        q.extend_from_slice(&0x0100u16.to_be_bytes()); // RD=1
        q.extend_from_slice(&1u16.to_be_bytes()); // QDCOUNT
        q.extend_from_slice(&[0, 0, 0, 0, 0, 0]); // AN/NS/AR = 0
        for label in name.split('.') {
            q.push(label.len() as u8);
            q.extend_from_slice(label.as_bytes());
        }
        q.push(0);
        q.extend_from_slice(&qtype.to_be_bytes());
        q.extend_from_slice(&CLASS_IN.to_be_bytes());
        q
    }

    #[test]
    fn parses_question_name_and_type() -> TestResult {
        let raw = make_query(0xABCD, "connectivitycheck.gstatic.com", TYPE_A);
        let query = parse_query(&raw)?;
        assert_eq!(query.id, 0xABCD);
        assert_eq!(query.name, "connectivitycheck.gstatic.com");
        assert_eq!(query.qtype, TYPE_A);
        assert_eq!(query.qclass, CLASS_IN);
        Ok(())
    }

    #[test]
    fn name_is_lowercased() -> TestResult {
        let raw = make_query(1, "SOS.Guide", TYPE_A);
        assert_eq!(parse_query(&raw)?.name, "sos.guide");
        Ok(())
    }

    #[test]
    fn a_query_answers_with_node_ip() -> TestResult {
        let raw = make_query(0x1234, "anything.test", TYPE_A);
        let query = parse_query(&raw)?;
        let resp = build_response(&query, Ipv4Addr::new(10, 0, 0, 1));
        // En-tête : ID recopié ; QR+AA (0x8400) + RD recopié (0x0100) = 0x8500 ;
        // ANCOUNT=1. Le bit RD est dans l'octet haut des flags.
        assert_eq!(resp.get(0..2), Some([0x12, 0x34].as_slice()));
        assert_eq!(resp.get(2..4), Some([0x85, 0x00].as_slice()));
        assert_eq!(resp.get(6..8), Some([0x00, 0x01].as_slice())); // ANCOUNT
                                                                   // Les 4 derniers octets = l'IP renvoyée.
        let n = resp.len();
        assert_eq!(resp.get(n - 4..n), Some([10, 0, 0, 1].as_slice()));
        Ok(())
    }

    #[test]
    fn rd_bit_is_echoed() -> TestResult {
        let raw = make_query(1, "x.test", TYPE_A);
        let resp = build_response(&parse_query(&raw)?, Ipv4Addr::LOCALHOST);
        // RD=1 dans la requête (bit 0x0100) ⇒ flags de réponse = 0x8500.
        assert_eq!(resp.get(2..4), Some([0x85, 0x00].as_slice()));
        Ok(())
    }

    #[test]
    fn aaaa_query_has_no_answer() -> TestResult {
        let raw = make_query(7, "ipv6.test", TYPE_AAAA);
        let resp = build_response(&parse_query(&raw)?, Ipv4Addr::new(10, 0, 0, 1));
        assert_eq!(resp.get(6..8), Some([0x00, 0x00].as_slice())); // ANCOUNT=0
        Ok(())
    }

    #[test]
    fn truncated_message_is_rejected() {
        assert_eq!(parse_query(&[0, 0, 1]), Err(DnsError::Truncated));
    }

    #[test]
    fn zero_question_is_rejected() {
        let mut raw = make_query(1, "x.test", TYPE_A);
        // Force QDCOUNT=0.
        if let Some(slot) = raw.get_mut(4..6) {
            slot.copy_from_slice(&0u16.to_be_bytes());
        }
        assert_eq!(parse_query(&raw), Err(DnsError::NoQuestion));
    }

    #[test]
    fn compression_pointer_in_question_is_rejected() {
        let mut raw = Vec::new();
        raw.extend_from_slice(&1u16.to_be_bytes());
        raw.extend_from_slice(&0x0100u16.to_be_bytes());
        raw.extend_from_slice(&1u16.to_be_bytes());
        raw.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
        raw.extend_from_slice(&[0xC0, 0x0C]); // pointeur déguisé en label
        assert_eq!(parse_query(&raw), Err(DnsError::BadName));
    }
}
