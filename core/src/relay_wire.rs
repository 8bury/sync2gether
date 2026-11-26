use serde::{Deserialize, Serialize};

pub const VERSION: u32 = 1;
pub const PORT: u16 = 7843;
pub const MAX_CONTROL: usize = 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Request {
    Host {
        version: u32,
        room: Option<String>,
        secret: String,
    },
    Alive,
    Accept {
        version: u32,
        room: String,
        secret: String,
        ticket: u64,
    },
    Join {
        version: u32,
        room: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Reply {
    Hosting { room: String },
    Incoming { ticket: u64 },
    Alive,
    Paired,
    Unknown,
    Away,
    Taken,
    Limited,
    Outdated { version: u32 },
}

pub fn valid_room(room: &str) -> bool {
    (4..=8).contains(&room.len())
        && room.bytes().all(|b| b.is_ascii_digit())
        && !room.starts_with('0')
}
pub fn valid_secret(secret: &str) -> bool {
    secret.len() == 32 && secret.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rooms_and_secrets_are_validated() {
        for room in ["1000", "4821", "99999999"] {
            assert!(valid_room(room));
        }
        for room in ["", "123", "0123", "123456789", "12a4", " 1234"] {
            assert!(!valid_room(room));
        }
        assert!(valid_secret(&"a".repeat(32)));
        assert!(!valid_secret(&"g".repeat(32)));
        assert!(!valid_secret(&"a".repeat(31)));
    }

    #[test]
    fn join_keeps_its_wire_shape() {
        let join = serde_json::to_string(&Request::Join {
            version: VERSION,
            room: "4821".into(),
        })
        .unwrap();
        assert_eq!(join, r#"{"Join":{"version":1,"room":"4821"}}"#);
        let paired: Reply = serde_json::from_str(r#""Paired""#).unwrap();
        assert_eq!(paired, Reply::Paired);
    }
}
