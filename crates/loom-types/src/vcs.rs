#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
}

impl ChangeKind {
    /// The stable numeric wire/ordering tag for this change kind. Matches the canonical ordering used
    /// by the watch cursor's change-kind canonicalization (Added < Modified < Deleted).
    pub const fn stable_tag(self) -> u8 {
        match self {
            ChangeKind::Added => 0,
            ChangeKind::Modified => 1,
            ChangeKind::Deleted => 2,
        }
    }

    /// The change kind for a stable tag, or `None` for an unknown tag.
    pub const fn from_stable_tag(tag: u8) -> Option<Self> {
        Some(match tag {
            0 => ChangeKind::Added,
            1 => ChangeKind::Modified,
            2 => ChangeKind::Deleted,
            _ => return None,
        })
    }
}
