//! Estados apresentados pela interface inicial.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SessionState {
    #[default]
    Idle,
    Waiting,
    Synchronized,
    Paused,
    Reconnecting,
}

impl SessionState {
    pub const ALL: [Self; 5] = [
        Self::Idle,
        Self::Waiting,
        Self::Synchronized,
        Self::Paused,
        Self::Reconnecting,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "Sem sessão",
            Self::Waiting => "Esperando parceiro",
            Self::Synchronized => "Sincronizados",
            Self::Paused => "Pausado",
            Self::Reconnecting => "Reconectando",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Action {
    SetDemoState(SessionState),
}
