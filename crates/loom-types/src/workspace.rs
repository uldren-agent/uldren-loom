use crate::error::{LoomError, Result};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum FacetKind {
    Files,
    Vcs,
    Sql,
    Kv,
    Document,
    Vector,
    Graph,
    Columnar,
    Queue,
    TimeSeries,
    Cas,
    Ledger,
    Program,
    Calendar,
    Contacts,
    Mail,
    Search,
    Dataframe,
    Metrics,
    Logs,
    Traces,
}

impl FacetKind {
    pub const ALL: [FacetKind; 21] = [
        FacetKind::Files,
        FacetKind::Vcs,
        FacetKind::Sql,
        FacetKind::Kv,
        FacetKind::Document,
        FacetKind::Vector,
        FacetKind::Graph,
        FacetKind::Columnar,
        FacetKind::Queue,
        FacetKind::TimeSeries,
        FacetKind::Cas,
        FacetKind::Ledger,
        FacetKind::Program,
        FacetKind::Calendar,
        FacetKind::Contacts,
        FacetKind::Mail,
        FacetKind::Search,
        FacetKind::Dataframe,
        FacetKind::Metrics,
        FacetKind::Logs,
        FacetKind::Traces,
    ];

    pub const fn stable_tag(self) -> u8 {
        match self {
            FacetKind::Files => 0,
            FacetKind::Sql => 1,
            FacetKind::Kv => 2,
            FacetKind::Graph => 3,
            FacetKind::Vector => 4,
            FacetKind::Columnar => 5,
            FacetKind::Ledger => 6,
            FacetKind::TimeSeries => 7,
            FacetKind::Document => 8,
            FacetKind::Cas => 9,
            FacetKind::Queue => 10,
            FacetKind::Calendar => 11,
            FacetKind::Contacts => 12,
            FacetKind::Mail => 13,
            FacetKind::Program => 14,
            FacetKind::Search => 15,
            FacetKind::Vcs => 16,
            FacetKind::Dataframe => 17,
            FacetKind::Metrics => 18,
            FacetKind::Logs => 19,
            FacetKind::Traces => 20,
        }
    }

    pub const fn from_stable_tag(tag: u8) -> Option<Self> {
        Some(match tag {
            0 => FacetKind::Files,
            1 => FacetKind::Sql,
            2 => FacetKind::Kv,
            3 => FacetKind::Graph,
            4 => FacetKind::Vector,
            5 => FacetKind::Columnar,
            6 => FacetKind::Ledger,
            7 => FacetKind::TimeSeries,
            8 => FacetKind::Document,
            9 => FacetKind::Cas,
            10 => FacetKind::Queue,
            11 => FacetKind::Calendar,
            12 => FacetKind::Contacts,
            13 => FacetKind::Mail,
            14 => FacetKind::Program,
            15 => FacetKind::Search,
            16 => FacetKind::Vcs,
            17 => FacetKind::Dataframe,
            18 => FacetKind::Metrics,
            19 => FacetKind::Logs,
            20 => FacetKind::Traces,
            _ => return None,
        })
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            FacetKind::Files => "files",
            FacetKind::Vcs => "vcs",
            FacetKind::Sql => "sql",
            FacetKind::Kv => "kv",
            FacetKind::Document => "document",
            FacetKind::Vector => "vector",
            FacetKind::Graph => "graph",
            FacetKind::Columnar => "columnar",
            FacetKind::Queue => "queue",
            FacetKind::TimeSeries => "time-series",
            FacetKind::Cas => "cas",
            FacetKind::Ledger => "ledger",
            FacetKind::Program => "program",
            FacetKind::Calendar => "calendar",
            FacetKind::Contacts => "contacts",
            FacetKind::Mail => "mail",
            FacetKind::Search => "search",
            FacetKind::Dataframe => "dataframe",
            FacetKind::Metrics => "metrics",
            FacetKind::Logs => "logs",
            FacetKind::Traces => "traces",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        Ok(match value {
            "files" => FacetKind::Files,
            "vcs" => FacetKind::Vcs,
            "sql" => FacetKind::Sql,
            "kv" => FacetKind::Kv,
            "document" => FacetKind::Document,
            "vector" => FacetKind::Vector,
            "graph" => FacetKind::Graph,
            "columnar" => FacetKind::Columnar,
            "queue" => FacetKind::Queue,
            "time-series" => FacetKind::TimeSeries,
            "cas" => FacetKind::Cas,
            "ledger" => FacetKind::Ledger,
            "program" => FacetKind::Program,
            "calendar" => FacetKind::Calendar,
            "contacts" => FacetKind::Contacts,
            "mail" => FacetKind::Mail,
            "search" => FacetKind::Search,
            "dataframe" => FacetKind::Dataframe,
            "metrics" => FacetKind::Metrics,
            "logs" => FacetKind::Logs,
            "traces" => FacetKind::Traces,
            other => {
                return Err(LoomError::invalid(format!(
                    "unknown workspace facet {other:?}"
                )));
            }
        })
    }

    pub const fn is_mountable(self) -> bool {
        self.supports_file_projection()
    }

    pub const fn supports_file_projection(self) -> bool {
        matches!(self, FacetKind::Files)
    }

    pub const fn supports_file_write_projection(self) -> bool {
        matches!(self, FacetKind::Files)
    }
}

impl fmt::Display for FacetKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum AclDomain {
    Files,
    Vcs,
    Sql,
    Kv,
    Document,
    Vector,
    Graph,
    Columnar,
    Queue,
    TimeSeries,
    Cas,
    Ledger,
    Program,
    Calendar,
    Contacts,
    Mail,
    Search,
    Dataframe,
    Metrics,
    Logs,
    Traces,
    Tickets,
    Pages,
    Chat,
    Lifecycle,
    Meetings,
}

impl AclDomain {
    pub const ALL: [Self; 26] = [
        Self::Files,
        Self::Vcs,
        Self::Sql,
        Self::Kv,
        Self::Document,
        Self::Vector,
        Self::Graph,
        Self::Columnar,
        Self::Queue,
        Self::TimeSeries,
        Self::Cas,
        Self::Ledger,
        Self::Program,
        Self::Calendar,
        Self::Contacts,
        Self::Mail,
        Self::Search,
        Self::Dataframe,
        Self::Metrics,
        Self::Logs,
        Self::Traces,
        Self::Tickets,
        Self::Pages,
        Self::Chat,
        Self::Lifecycle,
        Self::Meetings,
    ];

    pub const fn stable_tag(self) -> u8 {
        match self {
            Self::Files => 0,
            Self::Sql => 1,
            Self::Kv => 2,
            Self::Graph => 3,
            Self::Vector => 4,
            Self::Columnar => 5,
            Self::Ledger => 6,
            Self::TimeSeries => 7,
            Self::Document => 8,
            Self::Cas => 9,
            Self::Queue => 10,
            Self::Calendar => 11,
            Self::Contacts => 12,
            Self::Mail => 13,
            Self::Program => 14,
            Self::Search => 15,
            Self::Vcs => 16,
            Self::Dataframe => 17,
            Self::Metrics => 18,
            Self::Logs => 19,
            Self::Traces => 20,
            Self::Tickets => 21,
            Self::Pages => 22,
            Self::Chat => 23,
            Self::Lifecycle => 24,
            Self::Meetings => 25,
        }
    }

    pub const fn from_stable_tag(tag: u8) -> Option<Self> {
        Some(match tag {
            0 => Self::Files,
            1 => Self::Sql,
            2 => Self::Kv,
            3 => Self::Graph,
            4 => Self::Vector,
            5 => Self::Columnar,
            6 => Self::Ledger,
            7 => Self::TimeSeries,
            8 => Self::Document,
            9 => Self::Cas,
            10 => Self::Queue,
            11 => Self::Calendar,
            12 => Self::Contacts,
            13 => Self::Mail,
            14 => Self::Program,
            15 => Self::Search,
            16 => Self::Vcs,
            17 => Self::Dataframe,
            18 => Self::Metrics,
            19 => Self::Logs,
            20 => Self::Traces,
            21 => Self::Tickets,
            22 => Self::Pages,
            23 => Self::Chat,
            24 => Self::Lifecycle,
            25 => Self::Meetings,
            _ => return None,
        })
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Files => "files",
            Self::Vcs => "vcs",
            Self::Sql => "sql",
            Self::Kv => "kv",
            Self::Document => "document",
            Self::Vector => "vector",
            Self::Graph => "graph",
            Self::Columnar => "columnar",
            Self::Queue => "queue",
            Self::TimeSeries => "time-series",
            Self::Cas => "cas",
            Self::Ledger => "ledger",
            Self::Program => "program",
            Self::Calendar => "calendar",
            Self::Contacts => "contacts",
            Self::Mail => "mail",
            Self::Search => "search",
            Self::Dataframe => "dataframe",
            Self::Metrics => "metrics",
            Self::Logs => "logs",
            Self::Traces => "traces",
            Self::Tickets => "tickets",
            Self::Pages => "pages",
            Self::Chat => "chat",
            Self::Lifecycle => "lifecycle",
            Self::Meetings => "meetings",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|domain| domain.as_str() == value)
            .ok_or_else(|| LoomError::invalid(format!("unknown ACL domain {value:?}")))
    }
}

impl From<FacetKind> for AclDomain {
    fn from(facet: FacetKind) -> Self {
        Self::from_stable_tag(facet.stable_tag()).expect("every facet has an ACL domain")
    }
}

impl fmt::Display for AclDomain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorkspaceId([u8; 16]);

impl WorkspaceId {
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    pub const fn v4_from_bytes(mut bytes: [u8; 16]) -> Self {
        bytes[6] = (bytes[6] & 0x0f) | 0x40;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        Self(bytes)
    }

    pub fn parse(value: &str) -> Result<Self> {
        let hex_value: String = value.chars().filter(|c| *c != '-').collect();
        let raw = hex::decode(&hex_value)
            .map_err(|e| LoomError::invalid(format!("bad workspace id: {e}")))?;
        let bytes: [u8; 16] = raw
            .as_slice()
            .try_into()
            .map_err(|_| LoomError::invalid("workspace id must be 16 bytes"))?;
        Ok(Self(bytes))
    }
}

impl fmt::Display for WorkspaceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let b = &self.0;
        write!(
            f,
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            b[0],
            b[1],
            b[2],
            b[3],
            b[4],
            b[5],
            b[6],
            b[7],
            b[8],
            b[9],
            b[10],
            b[11],
            b[12],
            b[13],
            b[14],
            b[15]
        )
    }
}

impl fmt::Debug for WorkspaceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "WorkspaceId({self})")
    }
}
