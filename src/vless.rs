use anyhow::{anyhow, Result};
use std::fmt::Display;
use std::net::{Ipv4Addr, Ipv6Addr};
use url::Url;
use uuid::Uuid;

const VLESS_VERSION: u8 = 0; // VLESS version 0

#[derive(Debug, Clone, Default)]
pub struct VlessUrl {
    pub uuid: String,
    pub domain: String,
    pub port: u16,
    pub encryption: Option<String>,
    pub security: Option<String>,
    pub sni: Option<String>,
    pub alpn: Option<String>,
    pub r#type: Option<String>,
    pub host: Option<String>,
    pub path: Option<String>,
    pub comment: String,
}

impl From<VlessUrl> for Url {
    fn from(val: VlessUrl) -> Self {
        macro_rules! append_pair {
            ($url:expr, $key:expr, $value:expr) => {
                if let Some(v) = $value {
                    $url.query_pairs_mut().append_pair($key, &v);
                }
            };
        }

        let mut url = Url::parse(&format!("vless://{}@{}:{}", val.uuid, val.domain, val.port)).unwrap();
        append_pair!(url, "encryption", val.encryption);
        append_pair!(url, "security", val.security);
        append_pair!(url, "sni", val.sni);
        append_pair!(url, "alpn", val.alpn);
        append_pair!(url, "type", val.r#type);
        append_pair!(url, "host", val.host);
        append_pair!(url, "path", val.path);
        url.set_fragment(Some(&val.comment));
        url
    }
}

#[derive(Debug, PartialEq)]
pub enum VlessCommand {
    Tcp,
    Udp,
}

#[derive(Debug)]
pub enum VlessAddress {
    IPv4(Ipv4Addr),
    DomainName(String),
    IPv6(Ipv6Addr),
}

impl Display for VlessAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VlessAddress::IPv4(addr) => write!(f, "{addr}"),
            VlessAddress::DomainName(name) => write!(f, "{name}"),
            VlessAddress::IPv6(addr) => write!(f, "{addr}"),
        }
    }
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct VlessHeader {
    pub version: u8,
    pub command: VlessCommand,
    pub port: u16,
    pub opt_data: Vec<u8>,
    pub address: VlessAddress,
    pub len: usize, // length of the header
}

pub fn process_vless_header(header_bytes: &[u8], user_id: Uuid) -> Result<VlessHeader> {
    // https://xtls.github.io/development/protocols/vless.html
    // https://xtls.github.io/development/protocols/vmess.html
    use bytes::{Buf, Bytes};
    let mut bytes = Bytes::copy_from_slice(header_bytes);

    // test minimal length
    if bytes.len() < 26 {
        return Err(anyhow!("VLESS header too short"));
    }

    // read version
    let version = bytes.get_u8();
    if version != VLESS_VERSION {
        return Err(anyhow!("Unsupported VLESS version: {}", version));
    }

    // read and test uuid
    let uuid = Uuid::from_slice(&bytes.chunk()[..16])?;
    if uuid != user_id {
        return Err(anyhow!("UUID mismatch: expected {}, got {}", user_id, uuid));
    }
    bytes.advance(16);

    // read optional data
    let opt_length = bytes.get_u8();
    if bytes.len() < opt_length as usize {
        return Err(anyhow!("VLESS header too short for optional data"));
    }
    let opt_data = bytes.chunk()[..opt_length as usize].to_vec();
    bytes.advance(opt_length as usize);

    // read command
    let command_byte = bytes.try_get_u8()?;
    let command = match command_byte {
        1 => VlessCommand::Tcp, // TCP
        2 => VlessCommand::Udp, // UDP
        _ => return Err(anyhow!("Unsupported command: {}", command_byte)),
    };

    // read port
    let port = bytes.try_get_u16()?;

    // read address
    let address_type = bytes.try_get_u8()?;
    let address = match address_type {
        1 => {
            // IPv4
            if bytes.len() < 4 {
                return Err(anyhow!("VLESS header too short for IPv4 address"));
            }
            VlessAddress::IPv4(Ipv4Addr::new(bytes.get_u8(), bytes.get_u8(), bytes.get_u8(), bytes.get_u8()))
        }
        2 => {
            // Domain name
            let domain_name_length = bytes.try_get_u8()? as usize;
            if bytes.len() < domain_name_length {
                return Err(anyhow!("VLESS header too short for domain name"));
            }
            let domain = String::from_utf8(bytes.chunk()[..domain_name_length].to_vec())?;
            bytes.advance(domain_name_length);
            VlessAddress::DomainName(domain)
        }
        3 => {
            // IPv6
            if bytes.len() < 16 {
                return Err(anyhow!("VLESS header too short for IPv6 address"));
            }
            let ipv6_bytes: [u8; 16] = bytes.chunk()[..16].try_into()?;
            bytes.advance(16);
            VlessAddress::IPv6(Ipv6Addr::from(ipv6_bytes))
        }
        _ => return Err(anyhow!("Invalid address type: {}", address_type)),
    };

    let len = header_bytes.len() - bytes.len();
    Ok(VlessHeader { version, command, port, opt_data, address, len })
}

pub fn get_vless_response_header() -> Vec<u8> {
    vec![VLESS_VERSION, 0] // VLESS response header
}
