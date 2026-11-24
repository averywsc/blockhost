use sysinfo::System;

#[derive(Debug, Clone)]
pub struct Hardware {
    pub total_ram_mb: u64,
    pub cpu_cores: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub heap_mb: u64,
    pub max_players: u32,
    pub view_distance: u8,
    pub simulation_distance: u8,
    pub tier: &'static str,
    pub warning: Option<&'static str>,
}

pub fn detect() -> Hardware {
    let mut sys = System::new();
    sys.refresh_memory();
    Hardware {
        total_ram_mb: sys.total_memory() / (1024 * 1024),
        cpu_cores: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
    }
}

/// Pick a safe JVM heap and server limits for this machine.
/// The heap leaves headroom for the OS, since swapping kills Minecraft servers.
pub fn recommend(hw: &Hardware) -> Profile {
    let reserve = if hw.total_ram_mb <= 4096 { 1536 } else { 2048 };
    let heap_mb = hw
        .total_ram_mb
        .saturating_sub(reserve)
        .min(hw.total_ram_mb * 6 / 10)
        .clamp(1024, 12288);

    let (tier, mut players, view, sim) = match heap_mb {
        0..=1535 => ("potato", 4, 6, 4),
        1536..=3071 => ("light", 8, 8, 6),
        3072..=6143 => ("standard", 16, 10, 8),
        _ => ("beefy", 32, 12, 10),
    };
    if hw.cpu_cores <= 2 {
        players /= 2;
    }
    let warning = if hw.total_ram_mb < 3072 {
        Some("Under 3 GB RAM: expect a small server (a few friends). Close other apps.")
    } else if hw.cpu_cores <= 1 {
        Some("Single CPU core: the server will lag with more than a couple of players.")
    } else {
        None
    };
    Profile {
        heap_mb,
        max_players: players.max(2),
        view_distance: view,
        simulation_distance: sim,
        tier,
        warning,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hw(ram: u64, cores: usize) -> Hardware {
        Hardware {
            total_ram_mb: ram,
            cpu_cores: cores,
        }
    }

    #[test]
    fn old_4gb_laptop_gets_light_profile() {
        let p = recommend(&hw(4096, 4));
        assert_eq!(p.heap_mb, 2457);
        assert_eq!(p.tier, "light");
        assert!(p.heap_mb <= 4096 * 6 / 10);
    }

    #[test]
    fn tiny_machine_is_floored_and_warned() {
        let p = recommend(&hw(2048, 2));
        assert_eq!(p.heap_mb, 1024);
        assert!(p.warning.is_some());
        assert!(p.max_players >= 2);
    }

    #[test]
    fn big_machine_is_capped() {
        let p = recommend(&hw(65536, 16));
        assert_eq!(p.heap_mb, 12288);
        assert_eq!(p.tier, "beefy");
    }

    #[test]
    fn few_cores_halve_players() {
        assert!(recommend(&hw(8192, 2)).max_players < recommend(&hw(8192, 8)).max_players);
    }
}
