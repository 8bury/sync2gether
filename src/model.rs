#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SessionState {
    #[default]
    Idle,
    JoinCode,
    Browsing,
    Connecting,
    PendingApproval,
    Waiting,
    Approval,
    Choosing,
    Verifying,
    Mismatch,
    Stabilizing,
    PlayerError,
    Ready,
    Synchronized,
    Paused,
    Reconnecting,
    PeerLeft,
    RoomClosed,
}

impl SessionState {
    pub const ALL: [Self; 18] = [
        Self::Idle,
        Self::JoinCode,
        Self::Browsing,
        Self::Connecting,
        Self::PendingApproval,
        Self::Waiting,
        Self::Approval,
        Self::Choosing,
        Self::Verifying,
        Self::Mismatch,
        Self::Stabilizing,
        Self::PlayerError,
        Self::Ready,
        Self::Synchronized,
        Self::Paused,
        Self::Reconnecting,
        Self::PeerLeft,
        Self::RoomClosed,
    ];

    pub fn label(self, t: &'static crate::i18n::Strings) -> &'static str {
        let t = &t.states;
        match self {
            Self::Idle => t.idle,
            Self::JoinCode => t.join_code,
            Self::Browsing => t.browsing,
            Self::Connecting => t.connecting,
            Self::PendingApproval => t.pending_approval,
            Self::Approval => t.approval,
            Self::Choosing => t.choosing,
            Self::Verifying => t.verifying,
            Self::Mismatch => t.mismatch,
            Self::Stabilizing => t.stabilizing,
            Self::PlayerError => t.player_error,
            Self::Ready => t.ready,
            Self::Waiting => t.waiting,
            Self::Synchronized => t.synchronized,
            Self::Paused => t.paused,
            Self::Reconnecting => t.reconnecting,
            Self::PeerLeft => t.peer_left,
            Self::RoomClosed => t.room_closed,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Action {
    SetDemoState(SessionState),
}
