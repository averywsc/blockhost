//! Source RCON client over localhost (127.0.0.1) for communicating with a live Minecraft server.
//! Mirrors `panel.ps1` RCON behaviour: strict packet bounds, colour-code stripping, and timeouts.

use anyhow::{bail, Context, Result};
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    time::Duration,
};

const PACKET_TYPE_COMMAND_RESPONSE: i32 = 0;
const PACKET_TYPE_COMMAND: i32 = 2;
const PACKET_TYPE_AUTH_RESPONSE: i32 = 2;
const PACKET_TYPE_AUTH: i32 = 3;
const MAX_PACKET_LEN: usize = 8192;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RconResponse {
    pub text: String,
}

#[derive(Debug)]
struct Packet {
    id: i32,
    kind: i32,
    body: String,
}

fn encode_packet(id: i32, kind: i32, body: &str) -> Vec<u8> {
    let body_bytes = body.as_bytes();
    let len = (10 + body_bytes.len()) as i32;
    let mut out = Vec::with_capacity(4 + len as usize);
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&id.to_le_bytes());
    out.extend_from_slice(&kind.to_le_bytes());
    out.extend_from_slice(body_bytes);
    out.push(0);
    out.push(0);
    out
}

fn read_packet(stream: &mut TcpStream) -> Result<Packet> {
    let mut len_buf = [0u8; 4];
    stream
        .read_exact(&mut len_buf)
        .context("connection closed while reading RCON packet length")?;
    let len = i32::from_le_bytes(len_buf);
    if !(10..=MAX_PACKET_LEN as i32).contains(&len) {
        bail!("invalid RCON packet length: {len}");
    }
    let mut payload = vec![0u8; len as usize];
    stream
        .read_exact(&mut payload)
        .context("connection closed while reading RCON payload")?;
    let id = i32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
    let kind = i32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
    let body = String::from_utf8_lossy(&payload[8..payload.len() - 2]).into_owned();
    Ok(Packet { id, kind, body })
}

/// Strip Minecraft section-sign formatting codes (`§a`, `§l`, `§r`, etc.).
pub fn strip_mc_colors(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars();
    while let Some(c) = chars.next() {
        if c == '§' {
            let _ = chars.next();
        } else {
            out.push(c);
        }
    }
    out
}

/// Connect to `127.0.0.1:<port>`, authenticate with `password`, run `command`, and return the cleaned text.
/// Never connects off-localhost.
pub fn send_command(port: u16, password: &str, command: &str) -> Result<RconResponse> {
    if command.contains('\n') || command.contains('\r') {
        bail!("RCON command must be a single line");
    }
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(1500))
        .with_context(|| format!("could not connect to RCON on 127.0.0.1:{port}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(4)))?;
    stream.set_write_timeout(Some(Duration::from_secs(4)))?;

    // Authenticate
    let auth = encode_packet(1, PACKET_TYPE_AUTH, password);
    stream.write_all(&auth)?;
    stream.flush()?;

    let mut resp = read_packet(&mut stream)?;
    if resp.kind == PACKET_TYPE_AUTH_RESPONSE && resp.id == -1 {
        bail!("bad RCON password");
    }
    // Some servers send an empty COMMAND_RESPONSE before AUTH_RESPONSE.
    if resp.kind == PACKET_TYPE_COMMAND_RESPONSE {
        resp = read_packet(&mut stream)?;
        if resp.id == -1 {
            bail!("bad RCON password");
        }
    }

    // Send command
    let cmd = encode_packet(2, PACKET_TYPE_COMMAND, command);
    stream.write_all(&cmd)?;
    stream.flush()?;

    let text = match read_packet(&mut stream) {
        Ok(p) => strip_mc_colors(&p.body).trim().to_string(),
        Err(_) => String::new(),
    };
    Ok(RconResponse { text })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_minecraft_color_codes() {
        assert_eq!(strip_mc_colors("§aOK §fHello §lWorld§r"), "OK Hello World");
        assert_eq!(strip_mc_colors("plain text"), "plain text");
        assert_eq!(strip_mc_colors("trailing§"), "trailing");
    }

    #[test]
    fn rejects_multiline_commands() {
        assert!(send_command(25575, "pw", "say hi\nstop").is_err());
    }
}
