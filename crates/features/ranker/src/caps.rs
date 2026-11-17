use std::process::Command;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MachineCapabilities {
    /// Apple Silicon M3 and newer include hardware AV1 decode.
    pub hardware_av1: bool,
}

impl MachineCapabilities {
    pub fn detect() -> Self {
        static CACHED: OnceLock<MachineCapabilities> = OnceLock::new();
        *CACHED.get_or_init(detect_uncached)
    }
}

fn detect_uncached() -> MachineCapabilities {
    let output = Command::new("/usr/sbin/sysctl")
        .args(["-n", "machdep.cpu.brand_string", "hw.model"])
        .output();
    let Ok(output) = output else {
        return MachineCapabilities {
            hardware_av1: false,
        };
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let mut lines = text.lines();
    let brand = lines.next().unwrap_or("");
    let model = lines.next().unwrap_or("");
    MachineCapabilities {
        hardware_av1: parse_hardware_av1(brand, model),
    }
}

/// Determines hardware AV1 decode capability from `machdep.cpu.brand_string`
/// and `hw.model` (Apple M3+ / `Mac15,*`+ support hardware AV1).
pub fn parse_hardware_av1(brand: &str, model: &str) -> bool {
    if let Some(after) = brand.split("Apple M").nth(1) {
        let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(generation) = digits.parse::<u32>() {
            return generation >= 3;
        }
    }
    if let Some(after) = model.strip_prefix("Mac") {
        let major: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(num) = major.parse::<u32>() {
            return num >= 15;
        }
    }
    false
}
