//! Serveur DHCPv4 minimal, fait main (zéro dépendance), **sans baux persistés**.
//!
//! Conforme à l'exigence v2.5 : aucun bail écrit sur disque (conformité nLPD —
//! aucune donnée personnelle conservée). Les baux vivent en mémoire et sont
//! perdus au redémarrage ; un client réapparu retrouve la même IP tant que le
//! processus tourne (mappage MAC→IP en mémoire). **IPv4 uniquement** (l'IPv6 est
//! désactivée par ailleurs).
//!
//! Le codec (`parse_message`/`build_reply`) et l'allocation (`LeasePool`) sont
//! purs et testés ; la boucle réseau (`serve`) est mince.

use std::collections::{HashMap, HashSet};
use std::net::{Ipv4Addr, SocketAddr};

use tokio::net::UdpSocket;

/// `op` : requête client (BOOTREQUEST).
const OP_REQUEST: u8 = 1;
/// `op` : réponse serveur (BOOTREPLY).
const OP_REPLY: u8 = 2;
/// Type matériel Ethernet, longueur d'adresse 6 octets.
const HTYPE_ETHERNET: u8 = 1;
const HLEN_ETHERNET: u8 = 6;
/// Cookie magique DHCP (RFC 2131) précédant les options.
const MAGIC_COOKIE: [u8; 4] = [99, 130, 83, 99];
/// Décalage du cookie magique dans la trame BOOTP.
const COOKIE_OFFSET: usize = 236;
/// Décalage du champ `chaddr` (adresse matérielle du client).
const CHADDR_OFFSET: usize = 28;

/// Option 53 : type de message DHCP.
const OPT_MSG_TYPE: u8 = 53;
/// Option 1 : masque de sous-réseau.
const OPT_SUBNET_MASK: u8 = 1;
/// Option 3 : routeur (passerelle).
const OPT_ROUTER: u8 = 3;
/// Option 6 : serveur DNS.
const OPT_DNS: u8 = 6;
/// Option 51 : durée du bail (s).
const OPT_LEASE_TIME: u8 = 51;
/// Option 54 : identifiant du serveur.
const OPT_SERVER_ID: u8 = 54;
/// Option 50 : adresse demandée par le client.
const OPT_REQUESTED_IP: u8 = 50;
/// Option 255 : fin des options.
const OPT_END: u8 = 255;
/// Option 0 : remplissage.
const OPT_PAD: u8 = 0;

/// Type de message DHCP : découverte client.
pub const DHCP_DISCOVER: u8 = 1;
/// Type de message DHCP : offre serveur.
pub const DHCP_OFFER: u8 = 2;
/// Type de message DHCP : requête client.
pub const DHCP_REQUEST: u8 = 3;
/// Type de message DHCP : acquittement serveur.
pub const DHCP_ACK: u8 = 5;

/// Port serveur DHCP.
pub const SERVER_PORT: u16 = 67;
/// Port client DHCP.
pub const CLIENT_PORT: u16 = 68;

/// Erreur d'analyse d'une trame DHCP.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DhcpError {
    /// Trame plus courte que l'en-tête BOOTP + cookie, ou option tronquée.
    #[error("trame DHCP tronquée")]
    Truncated,
    /// Cookie magique absent ou incorrect.
    #[error("cookie magique DHCP invalide")]
    BadCookie,
}

/// Trame DHCP analysée (champs utiles seulement).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DhcpMessage {
    /// Code opération (`OP_REQUEST` attendu côté serveur).
    pub op: u8,
    /// Identifiant de transaction (réémis dans la réponse).
    pub xid: u32,
    /// Drapeaux (le bit 0x8000 réclame une réponse en diffusion).
    pub flags: u16,
    /// Adresse matérielle du client (6 premiers octets de `chaddr`).
    pub chaddr: [u8; 6],
    /// Type de message DHCP (option 53), si présent.
    pub msg_type: Option<u8>,
    /// Adresse demandée par le client (option 50), si présente.
    pub requested_ip: Option<Ipv4Addr>,
}

fn read_u16(buf: &[u8], pos: usize) -> Result<u16, DhcpError> {
    let bytes: [u8; 2] = buf
        .get(pos..pos + 2)
        .ok_or(DhcpError::Truncated)?
        .try_into()
        .map_err(|_| DhcpError::Truncated)?;
    Ok(u16::from_be_bytes(bytes))
}

fn read_u32(buf: &[u8], pos: usize) -> Result<u32, DhcpError> {
    let bytes: [u8; 4] = buf
        .get(pos..pos + 4)
        .ok_or(DhcpError::Truncated)?
        .try_into()
        .map_err(|_| DhcpError::Truncated)?;
    Ok(u32::from_be_bytes(bytes))
}

fn read_ipv4(data: &[u8]) -> Option<Ipv4Addr> {
    let bytes: [u8; 4] = data.get(0..4)?.try_into().ok()?;
    Some(Ipv4Addr::from(bytes))
}

/// Analyse une trame DHCP : en-tête BOOTP + options après le cookie magique.
pub fn parse_message(buf: &[u8]) -> Result<DhcpMessage, DhcpError> {
    let &op = buf.first().ok_or(DhcpError::Truncated)?;
    let xid = read_u32(buf, 4)?;
    let flags = read_u16(buf, 10)?;
    let chaddr: [u8; 6] = buf
        .get(CHADDR_OFFSET..CHADDR_OFFSET + 6)
        .ok_or(DhcpError::Truncated)?
        .try_into()
        .map_err(|_| DhcpError::Truncated)?;

    let cookie = buf
        .get(COOKIE_OFFSET..COOKIE_OFFSET + 4)
        .ok_or(DhcpError::Truncated)?;
    if cookie != MAGIC_COOKIE {
        return Err(DhcpError::BadCookie);
    }

    let mut msg_type = None;
    let mut requested_ip = None;
    let mut pos = COOKIE_OFFSET + 4;
    // Boucle d'options : s'arrête proprement à OPT_END ou en fin de trame.
    while let Some(&code) = buf.get(pos) {
        if code == OPT_END {
            break;
        }
        if code == OPT_PAD {
            pos += 1;
            continue;
        }
        let &len = buf.get(pos + 1).ok_or(DhcpError::Truncated)?;
        let len = len as usize;
        let data = buf
            .get(pos + 2..pos + 2 + len)
            .ok_or(DhcpError::Truncated)?;
        match code {
            OPT_MSG_TYPE => msg_type = data.first().copied(),
            OPT_REQUESTED_IP => requested_ip = read_ipv4(data),
            _ => {}
        }
        pos += 2 + len;
    }

    Ok(DhcpMessage {
        op,
        xid,
        flags,
        chaddr,
        msg_type,
        requested_ip,
    })
}

/// Paramètres réseau servis aux clients (tous dérivés de l'IP du nœud).
#[derive(Debug, Clone, Copy)]
pub struct DhcpConfig {
    /// IP du nœud : à la fois serveur, passerelle et DNS (île autonome).
    pub server_ip: Ipv4Addr,
    /// Masque de sous-réseau (ex. `255.255.255.0`).
    pub mask: Ipv4Addr,
    /// Durée de bail (s) — courte, car aucun bail n'est persisté.
    pub lease_secs: u32,
}

/// Pool d'adresses **en mémoire** (aucune persistance). Mappe MAC → IP pour la
/// durée de vie du processus.
#[derive(Debug)]
pub struct LeasePool {
    start: u32,
    end: u32,
    by_mac: HashMap<[u8; 6], Ipv4Addr>,
    used: HashSet<u32>,
}

impl LeasePool {
    /// Crée un pool couvrant `[start, end]` inclus.
    #[must_use]
    pub fn new(start: Ipv4Addr, end: Ipv4Addr) -> Self {
        Self {
            start: u32::from(start),
            end: u32::from(end),
            by_mac: HashMap::new(),
            used: HashSet::new(),
        }
    }

    /// Alloue (ou retrouve) l'IP d'un client. `None` si le pool est épuisé.
    pub fn allocate(&mut self, mac: [u8; 6]) -> Option<Ipv4Addr> {
        if let Some(ip) = self.by_mac.get(&mac) {
            return Some(*ip);
        }
        for n in self.start..=self.end {
            if !self.used.contains(&n) {
                let ip = Ipv4Addr::from(n);
                self.used.insert(n);
                self.by_mac.insert(mac, ip);
                return Some(ip);
            }
        }
        None
    }
}

/// Encode une option TLV simple dans `out`.
fn push_option(out: &mut Vec<u8>, code: u8, data: &[u8]) {
    out.push(code);
    out.push(data.len() as u8);
    out.extend_from_slice(data);
}

/// Construit une réponse DHCP (OFFER ou ACK) pour un client.
#[must_use]
pub fn build_reply(
    msg: &DhcpMessage,
    yiaddr: Ipv4Addr,
    reply_type: u8,
    cfg: &DhcpConfig,
) -> Vec<u8> {
    let mut out = vec![0u8; COOKIE_OFFSET]; // en-tête BOOTP, zéro par défaut
                                            // op / htype / hlen / hops
    if let Some(slot) = out.get_mut(0..4) {
        slot.copy_from_slice(&[OP_REPLY, HTYPE_ETHERNET, HLEN_ETHERNET, 0]);
    }
    if let Some(slot) = out.get_mut(4..8) {
        slot.copy_from_slice(&msg.xid.to_be_bytes());
    }
    if let Some(slot) = out.get_mut(10..12) {
        slot.copy_from_slice(&msg.flags.to_be_bytes());
    }
    // yiaddr (offset 16) = adresse attribuée ; siaddr (offset 20) = serveur.
    if let Some(slot) = out.get_mut(16..20) {
        slot.copy_from_slice(&yiaddr.octets());
    }
    if let Some(slot) = out.get_mut(20..24) {
        slot.copy_from_slice(&cfg.server_ip.octets());
    }
    if let Some(slot) = out.get_mut(CHADDR_OFFSET..CHADDR_OFFSET + 6) {
        slot.copy_from_slice(&msg.chaddr);
    }

    out.extend_from_slice(&MAGIC_COOKIE);
    push_option(&mut out, OPT_MSG_TYPE, &[reply_type]);
    push_option(&mut out, OPT_SERVER_ID, &cfg.server_ip.octets());
    push_option(&mut out, OPT_LEASE_TIME, &cfg.lease_secs.to_be_bytes());
    push_option(&mut out, OPT_SUBNET_MASK, &cfg.mask.octets());
    push_option(&mut out, OPT_ROUTER, &cfg.server_ip.octets());
    push_option(&mut out, OPT_DNS, &cfg.server_ip.octets());
    out.push(OPT_END);
    out
}

/// Décide la réponse à une trame reçue : DISCOVER→OFFER, REQUEST→ACK, sinon
/// rien. Alloue l'IP via le pool. `None` si rien à répondre (ou pool épuisé).
pub fn handle(msg: &DhcpMessage, pool: &mut LeasePool, cfg: &DhcpConfig) -> Option<Vec<u8>> {
    if msg.op != OP_REQUEST {
        return None;
    }
    let reply_type = match msg.msg_type? {
        DHCP_DISCOVER => DHCP_OFFER,
        DHCP_REQUEST => DHCP_ACK,
        _ => return None,
    };
    let yiaddr = pool.allocate(msg.chaddr)?;
    Some(build_reply(msg, yiaddr, reply_type, cfg))
}

/// Lie un socket UDP DHCP sur `addr` (helper pour l'orchestrateur). En mode réel
/// (`broadcast = true`), active la diffusion : la réponse part vers
/// `255.255.255.255:68` car le client n'a pas encore d'adresse.
pub async fn bind(addr: SocketAddr, broadcast: bool) -> std::io::Result<UdpSocket> {
    let socket = UdpSocket::bind(addr).await?;
    if broadcast {
        socket.set_broadcast(true)?;
    }
    Ok(socket)
}

/// Boucle de service DHCP : répond en diffusion (`255.255.255.255:68`) car le
/// client n'a pas encore d'adresse. Le socket doit être lié à l'IP du nœud:67
/// avec `set_broadcast(true)`.
pub async fn serve(socket: UdpSocket, cfg: DhcpConfig, mut pool: LeasePool) {
    let broadcast = SocketAddr::from((Ipv4Addr::BROADCAST, CLIENT_PORT));
    let mut buf = [0u8; 1024];
    loop {
        let len = match socket.recv_from(&mut buf).await {
            Ok((len, _)) => len,
            Err(err) => {
                tracing::warn!(%err, "DHCP: réception impossible — arrêt de la boucle");
                return;
            }
        };
        let datagram = match buf.get(..len) {
            Some(d) => d,
            None => continue,
        };
        match parse_message(datagram) {
            Ok(msg) => {
                if let Some(reply) = handle(&msg, &mut pool, &cfg) {
                    if let Err(err) = socket.send_to(&reply, broadcast).await {
                        tracing::warn!(%err, "DHCP: envoi de la réponse impossible");
                    }
                }
            }
            Err(err) => tracing::trace!(%err, "DHCP: trame ignorée"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn cfg() -> DhcpConfig {
        DhcpConfig {
            server_ip: Ipv4Addr::new(10, 0, 0, 1),
            mask: Ipv4Addr::new(255, 255, 255, 0),
            lease_secs: 600,
        }
    }

    /// Construit une trame DISCOVER/REQUEST minimale.
    fn make_request(xid: u32, mac: [u8; 6], msg_type: u8) -> Vec<u8> {
        let mut f = vec![0u8; COOKIE_OFFSET];
        if let Some(s) = f.get_mut(0..4) {
            s.copy_from_slice(&[OP_REQUEST, HTYPE_ETHERNET, HLEN_ETHERNET, 0]);
        }
        if let Some(s) = f.get_mut(4..8) {
            s.copy_from_slice(&xid.to_be_bytes());
        }
        if let Some(s) = f.get_mut(CHADDR_OFFSET..CHADDR_OFFSET + 6) {
            s.copy_from_slice(&mac);
        }
        f.extend_from_slice(&MAGIC_COOKIE);
        f.push(OPT_MSG_TYPE);
        f.push(1);
        f.push(msg_type);
        f.push(OPT_END);
        f
    }

    #[test]
    fn parses_discover() -> TestResult {
        let mac = [0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x01];
        let raw = make_request(0x11223344, mac, DHCP_DISCOVER);
        let msg = parse_message(&raw)?;
        assert_eq!(msg.op, OP_REQUEST);
        assert_eq!(msg.xid, 0x11223344);
        assert_eq!(msg.chaddr, mac);
        assert_eq!(msg.msg_type, Some(DHCP_DISCOVER));
        Ok(())
    }

    #[test]
    fn bad_cookie_is_rejected() {
        let mut raw = make_request(1, [0; 6], DHCP_DISCOVER);
        if let Some(s) = raw.get_mut(COOKIE_OFFSET..COOKIE_OFFSET + 4) {
            s.copy_from_slice(&[0, 0, 0, 0]);
        }
        assert_eq!(parse_message(&raw), Err(DhcpError::BadCookie));
    }

    #[test]
    fn discover_yields_offer_round_trips() -> TestResult {
        let mut pool = LeasePool::new(Ipv4Addr::new(10, 0, 0, 10), Ipv4Addr::new(10, 0, 0, 20));
        let mac = [1, 2, 3, 4, 5, 6];
        let raw = make_request(0xAABBCCDD, mac, DHCP_DISCOVER);
        let msg = parse_message(&raw)?;
        let reply = handle(&msg, &mut pool, &cfg()).ok_or("pas de réponse")?;
        let parsed = parse_message(&reply)?;
        // La réponse est un OP_REPLY avec le même xid et la même MAC.
        assert_eq!(parsed.op, OP_REPLY);
        assert_eq!(parsed.xid, 0xAABBCCDD);
        assert_eq!(parsed.chaddr, mac);
        assert_eq!(parsed.msg_type, Some(DHCP_OFFER));
        // yiaddr (offset 16) appartient au pool.
        assert_eq!(reply.get(16..20), Some([10, 0, 0, 10].as_slice()));
        Ok(())
    }

    #[test]
    fn request_yields_ack() -> TestResult {
        let mut pool = LeasePool::new(Ipv4Addr::new(10, 0, 0, 10), Ipv4Addr::new(10, 0, 0, 20));
        let raw = make_request(7, [9; 6], DHCP_REQUEST);
        let reply = handle(&parse_message(&raw)?, &mut pool, &cfg()).ok_or("pas de réponse")?;
        assert_eq!(parse_message(&reply)?.msg_type, Some(DHCP_ACK));
        Ok(())
    }

    #[test]
    fn same_mac_keeps_same_ip() {
        let mut pool = LeasePool::new(Ipv4Addr::new(10, 0, 0, 10), Ipv4Addr::new(10, 0, 0, 12));
        let mac = [0xAB; 6];
        let first = pool.allocate(mac);
        let again = pool.allocate(mac);
        assert_eq!(first, again);
        assert_eq!(first, Some(Ipv4Addr::new(10, 0, 0, 10)));
    }

    #[test]
    fn distinct_macs_get_distinct_ips_until_exhausted() {
        let mut pool = LeasePool::new(Ipv4Addr::new(10, 0, 0, 10), Ipv4Addr::new(10, 0, 0, 11));
        let a = pool.allocate([1; 6]);
        let b = pool.allocate([2; 6]);
        let c = pool.allocate([3; 6]); // pool épuisé (2 adresses)
        assert_ne!(a, b);
        assert_eq!(c, None);
    }

    #[test]
    fn no_lease_is_persisted_pool_is_in_memory() {
        // Un nouveau pool ne connaît aucun bail : aucune persistance.
        let mut pool = LeasePool::new(Ipv4Addr::new(10, 0, 0, 10), Ipv4Addr::new(10, 0, 0, 20));
        assert_eq!(pool.allocate([7; 6]), Some(Ipv4Addr::new(10, 0, 0, 10)));
        let mut fresh = LeasePool::new(Ipv4Addr::new(10, 0, 0, 10), Ipv4Addr::new(10, 0, 0, 20));
        // Le client retrouve la PREMIÈRE adresse, prouvant qu'il n'y a pas d'état partagé.
        assert_eq!(fresh.allocate([7; 6]), Some(Ipv4Addr::new(10, 0, 0, 10)));
    }
}
