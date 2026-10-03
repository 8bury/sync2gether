//! Estados apresentados pela interface inicial.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SessionState {
    #[default]
    Idle,
    Browsing,
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
}

impl SessionState {
    pub const ALL: [Self; 13] = [
        Self::Idle,
        Self::Browsing,
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
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "Sem sessão",
            Self::Browsing => "Lista de salas",
            Self::Approval => "Solicitação de entrada",
            Self::Choosing => "Escolhendo arquivos",
            Self::Verifying => "Verificando arquivos",
            Self::Mismatch => "Arquivos diferentes",
            Self::Stabilizing => "Estabilizando conexão",
            Self::PlayerError => "Erro no player",
            Self::Ready => "Prontos",
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
