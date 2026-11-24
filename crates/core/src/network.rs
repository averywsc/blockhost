//! Network diagnostics, ping statistics, LAN/public IP, Wi-Fi detection,
//! UPnP port forwarding (`igd-next`), CGNAT detection, and Windows Firewall helpers.

use crate::net;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket},
    process::Command,
    time::Duration,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PingStats {
    pub ok: bool,
    pub sent: usize,
    pub received: usize,
    pub avg_ms: Option<f64>,
    pub min_ms: Option<f64>,
    pub max_ms: Option<f64>,
    pub jitter_ms: Option<f64>,
    pub loss_pct: u8,
    pub grade: String,
    pub samples_ms: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LinkInfo {
    pub name: String,
    pub wifi: bool,
    pub gateway: String,
    pub speed: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CgnatReport {
    pub detected: bool,
    pub router_wan_ip: Option<String>,
    pub public_ip: Option<String>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActionResult {
    pub ok: bool,
    pub needs_admin: bool,
    pub message: String,
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

/// Compute average, min, max, jitter, packet loss, and plain-language grade matching `panel.ps1`.
pub fn compute_ping_stats(rtts: &[f64], sent: usize) -> PingStats {
    if sent == 0 || rtts.is_empty() {
        return PingStats {
            ok: false,
            sent,
            received: 0,
            avg_ms: None,
            min_ms: None,
            max_ms: None,
            jitter_ms: None,
            loss_pct: 100,
            grade: "No reply".to_string(),
            samples_ms: Vec::new(),
        };
    }

    let sum: f64 = rtts.iter().sum();
    let avg = round1(sum / rtts.len() as f64);
    let min = rtts.iter().copied().fold(f64::INFINITY, f64::min);
    let max = rtts.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let jitter = if rtts.len() > 1 {
        let diff_sum: f64 = rtts.windows(2).map(|w| (w[1] - w[0]).abs()).sum();
        round1(diff_sum / (rtts.len() - 1) as f64)
    } else {
        0.0
    };
    let lost = sent.saturating_sub(rtts.len());
    let loss_pct = ((100.0 * lost as f64) / sent as f64).round().clamp(0.0, 100.0) as u8;

    let mut stats = PingStats {
        ok: true,
        sent,
        received: rtts.len(),
        avg_ms: Some(avg),
        min_ms: Some(round1(min)),
        max_ms: Some(round1(max)),
        jitter_ms: Some(jitter),
        loss_pct,
        grade: String::new(),
        samples_ms: rtts.to_vec(),
    };
    stats.grade = grade_ping(&stats).to_string();
    stats
}

pub fn grade_ping(st: &PingStats) -> &'static str {
    if !st.ok {
        return "No reply";
    }
    if st.loss_pct >= 5 {
        return "Poor (packet loss - expect rubber-banding)";
    }
    let avg = st.avg_ms.unwrap_or(999.0);
    let jitter = st.jitter_ms.unwrap_or(999.0);
    if avg < 30.0 && jitter < 10.0 {
        "Excellent"
    } else if avg < 60.0 && jitter < 20.0 {
        "Good"
    } else if avg < 100.0 {
        "OK (noticeable in fast games)"
    } else {
        "Poor (high delay)"
    }
}

/// Validate a ping target hostname or IP (strips optional `:port` suffix first).
pub fn sanitize_ping_target(raw: &str) -> Result<String> {
    let trimmed = raw.trim();
    let host = match trimmed.rsplit_once(':') {
        Some((h, p)) if !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) => h,
        _ => trimmed,
    };
    if host.is_empty()
        || host.len() > 100
        || host.starts_with('-')
        || !host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
    {
        bail!("That does not look like a valid address or IP.");
    }
    Ok(host.to_string())
}

/// Extract RTT in milliseconds from one line of `ping` output (`time=12.3 ms` or `time<1ms`).
pub fn parse_ping_rtt_ms(output: &str) -> Option<f64> {
    let lower = output.to_ascii_lowercase();
    for line in lower.lines() {
        if let Some(pos) = line.find("time<") {
            let rest = &line[pos + 5..];
            let num: String = rest
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect();
            if let Ok(v) = num.parse::<f64>() {
                return Some((v * 0.5).max(0.5));
            }
        }
        if let Some(pos) = line.find("time=") {
            let rest = &line[pos + 5..];
            let num: String = rest
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect();
            if let Ok(v) = num.parse::<f64>() {
                return Some(v);
            }
        }
    }
    None
}

/// Ping `target` `count` times using the OS `ping` command (no admin/raw socket required).
pub fn ping_host(raw_target: &str, count: usize) -> Result<PingStats> {
    let target = sanitize_ping_target(raw_target)?;
    let count = count.clamp(1, 20);
    let mut rtts = Vec::with_capacity(count);
    for i in 0..count {
        let mut cmd = Command::new("ping");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
            cmd.args(["-n", "1", "-w", "1500", &target]);
        }
        #[cfg(not(windows))]
        {
            cmd.args(["-c", "1", "-W", "2", &target]);
        }
        if let Ok(out) = cmd.output() {
            if out.status.success() {
                let text = String::from_utf8_lossy(&out.stdout);
                if let Some(ms) = parse_ping_rtt_ms(&text) {
                    rtts.push(ms);
                }
            }
        }
        if i + 1 < count {
            std::thread::sleep(Duration::from_millis(120));
        }
    }
    Ok(compute_ping_stats(&rtts, count))
}

/// Determine this machine's primary LAN IPv4 address (never returns loopback or link-local).
pub fn detect_lan_ip() -> Option<Ipv4Addr> {
    let sock = UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("1.1.1.1:80").ok()?;
    match sock.local_addr().ok()?.ip() {
        IpAddr::V4(ip) if !ip.is_loopback() && !ip.is_link_local() && !ip.is_unspecified() => Some(ip),
        _ => None,
    }
}

/// Detect a local Tailscale IPv4 address (`100.64.0.0/10`) if Tailscale is installed & connected.
/// Tailscale gives direct peer-to-peer NZ ping (~5-15 ms) even behind ISP CGNAT with zero playit relay.
pub fn detect_tailscale_ip() -> Option<String> {
    let mut cmd = Command::new("tailscale");
    cmd.args(["ip", "-4"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines()
        .map(str::trim)
        .find_map(|l| l.parse::<Ipv4Addr>().ok().map(|ip| ip.to_string()))
}

/// Normalize a DuckDNS subdomain (strips `.duckdns.org` if included) and validate it.
pub fn normalize_duckdns_subdomain(raw: &str) -> Result<String> {
    let sub = raw
        .trim()
        .to_ascii_lowercase()
        .trim_end_matches(".duckdns.org")
        .to_string();
    if sub.is_empty()
        || sub.len() > 63
        || !sub
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        bail!("DuckDNS name can only use letters, numbers, and dashes.");
    }
    Ok(sub)
}

/// Update a free DuckDNS hostname (`<subdomain>.duckdns.org`) to point to this PC's current public IP.
pub fn update_duckdns(subdomain: &str, token: &str) -> Result<String> {
    let sub = normalize_duckdns_subdomain(subdomain)?;
    let token = token.trim();
    if token.len() < 8 || !token.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        bail!("DuckDNS token does not look valid.");
    }
    let url = format!("https://www.duckdns.org/update?domains={sub}&token={token}&ip=");
    let body = net::agent()
        .get(&url)
        .timeout(Duration::from_secs(10))
        .call()
        .context("Could not reach DuckDNS.")?
        .into_string()
        .unwrap_or_default();
    if body.trim().eq_ignore_ascii_case("OK") {
        Ok(format!("{sub}.duckdns.org"))
    } else {
        bail!("DuckDNS rejected the update (check your subdomain and token).");
    }
}

/// Fetch our observed public IP via ipify.
pub fn detect_public_ip() -> Option<String> {
    let text = net::agent()
        .get("https://api.ipify.org")
        .timeout(Duration::from_secs(5))
        .call()
        .ok()?
        .into_string()
        .ok()?;
    let ip = text.trim();
    ip.parse::<IpAddr>().ok().map(|a| a.to_string())
}

/// Detect whether an adapter is Wi-Fi based on its media type or description.
pub fn is_wifi_adapter(media: &str, desc: &str) -> bool {
    let m = media.to_ascii_lowercase();
    let d = desc.to_ascii_lowercase();
    m.contains("802.11")
        || d.contains("wi-fi")
        || d.contains("wifi")
        || d.contains("wireless")
        || d.contains("wlan")
}

/// Inspect active default-route adapter on Windows (name, Wi-Fi vs cable, gateway, link speed).
pub fn get_link_info() -> Option<LinkInfo> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let ps = "$rt = Get-NetRoute -DestinationPrefix '0.0.0.0/0' | Sort-Object RouteMetric | Select-Object -First 1; \
                  $ad = Get-NetAdapter -InterfaceIndex $rt.InterfaceIndex; \
                  @{ name=$ad.Name; media=\"$($ad.PhysicalMediaType)\"; desc=\"$($ad.InterfaceDescription)\"; gateway=\"$($rt.NextHop)\"; speed=\"$($ad.LinkSpeed)\" } | ConvertTo-Json -Compress";
        let out = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", ps])
            .creation_flags(0x0800_0000)
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        #[derive(Deserialize)]
        struct RawLink {
            name: String,
            media: String,
            desc: String,
            gateway: String,
            speed: String,
        }
        let parsed: RawLink = serde_json::from_slice(&out.stdout).ok()?;
        Some(LinkInfo {
            wifi: is_wifi_adapter(&parsed.media, &parsed.desc),
            name: parsed.name,
            gateway: parsed.gateway,
            speed: parsed.speed,
        })
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Check if an IPv4 address is in RFC 6598 CGNAT (`100.64.0.0/10`) or RFC 1918 private space.
pub fn is_non_public_ipv4(ip: Ipv4Addr) -> bool {
    let [a, b, _, _] = ip.octets();
    let is_cgnat = a == 100 && (64..=127).contains(&b);
    is_cgnat || ip.is_private() || ip.is_loopback() || ip.is_link_local()
}

/// Evaluate whether this internet connection is behind ISP CGNAT / double NAT.
pub fn evaluate_cgnat(router_wan_ip: Option<&str>, public_ip: Option<&str>) -> CgnatReport {
    let wan_v4 = router_wan_ip.and_then(|s| s.parse::<Ipv4Addr>().ok());
    let pub_v4 = public_ip.and_then(|s| s.parse::<Ipv4Addr>().ok());

    match (wan_v4, pub_v4) {
        (Some(wan), Some(pub_ip)) if is_non_public_ipv4(wan) => CgnatReport {
            detected: true,
            router_wan_ip: Some(wan.to_string()),
            public_ip: Some(pub_ip.to_string()),
            summary: format!(
                "Your internet provider uses shared addresses (CGNAT / double NAT — router sees {wan}, internet sees {pub_ip}). Direct port forwarding will not work; use the free playit.gg tunnel instead."
            ),
        },
        (Some(wan), Some(pub_ip)) if wan != pub_ip => CgnatReport {
            detected: true,
            router_wan_ip: Some(wan.to_string()),
            public_ip: Some(pub_ip.to_string()),
            summary: format!(
                "Your router's WAN IP ({wan}) does not match your public IP ({pub_ip}), which usually means CGNAT or a second router. Use the playit.gg tunnel, or ask your ISP for a public IP."
            ),
        },
        (Some(wan), Some(pub_ip)) => CgnatReport {
            detected: false,
            router_wan_ip: Some(wan.to_string()),
            public_ip: Some(pub_ip.to_string()),
            summary: format!(
                "No CGNAT detected — your router has a direct public IP ({pub_ip}). Direct port forwarding should give your NZ friends the lowest possible ping."
            ),
        },
        (None, Some(pub_ip)) => CgnatReport {
            detected: false,
            router_wan_ip: None,
            public_ip: Some(pub_ip.to_string()),
            summary: "Could not read your router's WAN IP via UPnP, so CGNAT could not be confirmed automatically. If port forwarding doesn't let friends join, use the playit.gg tunnel.".to_string(),
        },
        _ => CgnatReport {
            detected: false,
            router_wan_ip: router_wan_ip.map(String::from),
            public_ip: public_ip.map(String::from),
            summary: "Could not check your public internet address right now.".to_string(),
        },
    }
}

/// Discover UPnP gateway and query its external IP (if UPnP is enabled on the router).
pub fn upnp_external_ip() -> Result<String> {
    let opts = igd_next::SearchOptions {
        timeout: Some(Duration::from_secs(3)),
        ..Default::default()
    };
    let gw = igd_next::search_gateway(opts)
        .context("Your router did not answer UPnP (it may be turned off in router settings).")?;
    let ip = gw
        .get_external_ip()
        .context("Router did not return an external IP.")?;
    Ok(ip.to_string())
}

/// Add a UPnP TCP port forward for `port` to this PC's LAN IP. Never forwards RCON ports.
pub fn add_upnp_forward(port: u16) -> ActionResult {
    let Some(lan) = detect_lan_ip() else {
        return ActionResult {
            ok: false,
            needs_admin: false,
            message: "Could not work out this PC's home-network address.".into(),
        };
    };
    let opts = igd_next::SearchOptions {
        timeout: Some(Duration::from_secs(4)),
        ..Default::default()
    };
    let gw = match igd_next::search_gateway(opts) {
        Ok(g) => g,
        Err(_) => {
            return ActionResult {
                ok: false,
                needs_admin: false,
                message: "Your router did not answer. UPnP may be switched off in router settings (or your provider blocks it).".into(),
            }
        }
    };
    let local = SocketAddr::new(IpAddr::V4(lan), port);
    match gw.add_port(
        igd_next::PortMappingProtocol::TCP,
        port,
        local,
        0,
        "BlockHost Minecraft Server",
    ) {
        Ok(()) => ActionResult {
            ok: true,
            needs_admin: false,
            message: format!("Forwarded TCP port {port} to {lan}."),
        },
        Err(e) => ActionResult {
            ok: false,
            needs_admin: false,
            message: format!("Router refused UPnP port forward: {e}"),
        },
    }
}

/// Remove a previously created UPnP TCP port forward for `port`.
pub fn remove_upnp_forward(port: u16) -> ActionResult {
    let opts = igd_next::SearchOptions {
        timeout: Some(Duration::from_secs(4)),
        ..Default::default()
    };
    let gw = match igd_next::search_gateway(opts) {
        Ok(g) => g,
        Err(_) => {
            return ActionResult {
                ok: false,
                needs_admin: false,
                message: "Your router did not answer.".into(),
            }
        }
    };
    match gw.remove_port(igd_next::PortMappingProtocol::TCP, port) {
        Ok(()) => ActionResult {
            ok: true,
            needs_admin: false,
            message: format!("Removed the port forward for TCP port {port}."),
        },
        Err(e) => ActionResult {
            ok: false,
            needs_admin: false,
            message: format!("Could not remove port forward: {e}"),
        },
    }
}

/// Add a Windows Firewall inbound rule for the Minecraft game port (`port`, TCP only).
/// Never opens RCON ports. Returns `needs_admin = true` when not running elevated.
pub fn add_firewall_rule(port: u16, rcon_port: u16) -> ActionResult {
    if port == rcon_port {
        return ActionResult {
            ok: false,
            needs_admin: false,
            message: "Refusing to open the RCON port in the firewall for safety.".into(),
        };
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let check_ps = format!(
            "if (Get-NetFirewallRule -DisplayName 'BlockHost Minecraft Server ({port})' -ErrorAction SilentlyContinue) {{ exit 0 }} else {{ exit 1 }}"
        );
        if let Ok(st) = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &check_ps])
            .creation_flags(0x0800_0000)
            .status()
        {
            if st.success() {
                return ActionResult {
                    ok: true,
                    needs_admin: false,
                    message: format!("Windows Firewall already allows TCP port {port}."),
                };
            }
        }
        let add_ps = format!(
            "New-NetFirewallRule -DisplayName 'BlockHost Minecraft Server ({port})' -Direction Inbound -Protocol TCP -LocalPort {port} -Action Allow -Profile Private,Public | Out-Null"
        );
        match Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &add_ps])
            .creation_flags(0x0800_0000)
            .output()
        {
            Ok(out) if out.status.success() => ActionResult {
                ok: true,
                needs_admin: false,
                message: format!("Windows Firewall now allows TCP port {port}."),
            },
            _ => ActionResult {
                ok: false,
                needs_admin: true,
                message: "Adding a Windows Firewall rule requires administrator permission (UAC).".into(),
            },
        }
    }
    #[cfg(not(windows))]
    {
        ActionResult {
            ok: false,
            needs_admin: false,
            message: "Windows Firewall rules only apply on Windows.".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ping_stats_match_panel_spec() {
        let st = compute_ping_stats(&[10.0, 12.0, 11.0, 30.0], 4);
        assert!(st.ok);
        assert_eq!(st.avg_ms, Some(15.8));
        assert_eq!(st.min_ms, Some(10.0));
        assert_eq!(st.max_ms, Some(30.0));
        assert_eq!(st.jitter_ms, Some(7.3));
        assert_eq!(st.loss_pct, 0);
        assert_eq!(st.grade, "Excellent");

        let lost = compute_ping_stats(&[20.0, 22.0], 10);
        assert_eq!(lost.loss_pct, 80);
        assert!(lost.grade.starts_with("Poor (packet loss"));

        let none = compute_ping_stats(&[], 10);
        assert!(!none.ok);
        assert_eq!(none.loss_pct, 100);
        assert_eq!(none.grade, "No reply");

        let slow = compute_ping_stats(&[150.0, 152.0, 151.0], 3);
        assert!(slow.grade.starts_with("Poor (high delay"));
    }

    #[test]
    fn parses_windows_and_unix_ping_output() {
        assert_eq!(
            parse_ping_rtt_ms("Reply from 1.1.1.1: bytes=32 time=11ms TTL=57"),
            Some(11.0)
        );
        assert_eq!(
            parse_ping_rtt_ms("Reply from 127.0.0.1: bytes=32 time<1ms TTL=128"),
            Some(0.5)
        );
        assert_eq!(
            parse_ping_rtt_ms("64 bytes from 1.1.1.1: icmp_seq=1 ttl=57 time=14.6 ms"),
            Some(14.6)
        );
        assert_eq!(parse_ping_rtt_ms("Request timed out."), None);
    }

    #[test]
    fn wifi_adapter_detection() {
        assert!(is_wifi_adapter("Native 802.11", "Intel AX201"));
        assert!(is_wifi_adapter("", "Realtek Wireless LAN"));
        assert!(!is_wifi_adapter("802.3", "Realtek PCIe GbE"));
    }

    #[test]
    fn cgnat_detection_cases() {
        let r1 = evaluate_cgnat(Some("100.64.12.5"), Some("203.97.1.10"));
        assert!(r1.detected);

        let r2 = evaluate_cgnat(Some("192.168.0.2"), Some("203.97.1.10"));
        assert!(r2.detected);

        let r3 = evaluate_cgnat(Some("203.97.1.10"), Some("203.97.1.10"));
        assert!(!r3.detected);
    }

    #[test]
    fn firewall_refuses_to_open_rcon_port() {
        let r = add_firewall_rule(25575, 25575);
        assert!(!r.ok);
    }
}
