pub mod amp;
pub mod claude;
pub mod codex;
pub mod gemini;
pub mod opencode;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AdapterKind {
    Codex,
    Claude,
    Gemini,
    Amp,
    OpenCode,
}

impl AdapterKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Gemini => "gemini",
            Self::Amp => "amp",
            Self::OpenCode => "opencode",
        }
    }
}

#[must_use]
pub const fn all_adapter_kinds() -> [AdapterKind; 5] {
    [
        AdapterKind::Codex,
        AdapterKind::Claude,
        AdapterKind::Gemini,
        AdapterKind::Amp,
        AdapterKind::OpenCode,
    ]
}

#[must_use]
pub const fn default_paths(kind: AdapterKind) -> &'static [&'static str] {
    match kind {
        AdapterKind::Codex => codex::DEFAULT_PATHS,
        AdapterKind::Claude => claude::DEFAULT_PATHS,
        AdapterKind::Gemini => gemini::DEFAULT_PATHS,
        AdapterKind::Amp => amp::DEFAULT_PATHS,
        AdapterKind::OpenCode => opencode::DEFAULT_PATHS,
    }
}
