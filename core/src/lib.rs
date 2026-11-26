#[cfg(feature = "crypto")]
pub mod chunks;
pub mod code;
pub mod datachannel;
pub mod frame;
pub mod protocol;
pub mod relay_wire;
#[cfg(feature = "crypto")]
pub mod secure;
pub mod sync;

pub fn device_name(name: &str) -> String {
    let name: String = name.chars().filter(|c| !c.is_control()).take(64).collect();
    if name.trim().is_empty() {
        "Outro computador".into()
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_names_drop_control_characters_and_have_a_fallback() {
        assert_eq!(device_name("a\nb"), "ab");
        assert_eq!(device_name("\n\t"), "Outro computador");
        assert_eq!(device_name(&"x".repeat(100)).len(), 64);
    }
}
