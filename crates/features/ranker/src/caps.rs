#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MachineCapabilities {
    /// Apple Silicon M3 and newer include hardware AV1 decode.
    pub hardware_av1: bool,
}

impl MachineCapabilities {
    /// Builds capabilities from `machdep.cpu.brand_string` and `hw.model`
    /// (read in-process by the caller; the ranker stays platform-free).
    pub fn from_sysctl(brand: &str, model: &str) -> Self {
        Self {
            hardware_av1: parse_hardware_av1(brand, model),
        }
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
