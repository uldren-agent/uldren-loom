//! The MCP resource surface under the `loom://` and `ui://` schemes.
//!
//! Resources expose loom data as URIs a client can list, template, and read: workspace files, CAS
//! blobs by content address, calendar/contacts/mail bodies via `loom-vfs` codecs
//! (`.ics`/`.vcf`/`.eml`), and MCP App HTML resources. Parsing and the template catalog live here
//! without rmcp types; the rmcp `ServerHandler` resource methods in [`crate::server`] dispatch a parsed
//! target to the read facade. Every resource read runs through the engine policy enforcement point like
//! a read tool.
//!
//! Licensed under BUSL-1.1.

use crate::apps::AppTarget;

/// A parsed MCP resource URI, resolved to the facet or app read it addresses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResourceTarget {
    /// `loom://capabilities.json` - source-owned capability matrix.
    Capabilities,
    /// `loom://<workspace>/` - workspace metadata.
    Workspace { workspace: String },
    /// `loom://<workspace>/files/<path>` - a workspace file's bytes.
    File { workspace: String, path: String },
    /// `loom://<workspace>/cas/<digest>` - a CAS blob by content address.
    Cas { workspace: String, digest: String },
    /// `loom://<workspace>/calendar/<principal>/<collection>/<uid>.ics` - an event as iCalendar.
    CalendarIcs {
        workspace: String,
        principal: String,
        collection: String,
        uid: String,
    },
    /// `loom://<workspace>/contacts/<principal>/<book>/<uid>.vcf` - a contact as vCard.
    ContactsVcf {
        workspace: String,
        principal: String,
        book: String,
        uid: String,
    },
    /// `loom://<workspace>/mail/<principal>/<mailbox>/<uid>.eml` - a message as RFC 5322.
    MailEml {
        workspace: String,
        principal: String,
        mailbox: String,
        uid: String,
    },
    /// `loom://<workspace>/studio/views/status/principal/<principal>` - assistant status bootstrap.
    StudioStatus {
        workspace: String,
        principal: String,
    },
    /// `loom://<workspace>/substrate/views/<view_id>.json` - a substrate view definition.
    SubstrateView { workspace: String, view_id: String },
    /// `loom://<workspace>/substrate/refs/<target>.json` - inbound references for one target.
    SubstrateRefs { workspace: String, target: String },
    /// `ui://<workspace>/mcp/apps/<app>` - a Loom-backed MCP App HTML resource.
    App(AppTarget),
}

/// One advertised resource template (a parameterized `loom://` URI).
#[derive(Clone, Copy, Debug)]
pub struct ResourceTemplateSpec {
    /// The RFC 6570-style URI template.
    pub uri_template: &'static str,
    /// A short name.
    pub name: &'static str,
    /// A one-line description.
    pub description: &'static str,
    /// The MIME type of what a read returns.
    pub mime_type: &'static str,
}

/// The advertised resource templates (`resources/templates/list`).
pub const TEMPLATES: &[ResourceTemplateSpec] = &[
    ResourceTemplateSpec {
        uri_template: "loom://{workspace}/files/{path}",
        name: "file",
        description: "A workspace file's bytes.",
        mime_type: "application/octet-stream",
    },
    ResourceTemplateSpec {
        uri_template: "loom://{workspace}/cas/{digest}",
        name: "cas-blob",
        description: "A content-addressed blob (digest is the version/ETag).",
        mime_type: "application/octet-stream",
    },
    ResourceTemplateSpec {
        uri_template: "loom://{workspace}/calendar/{principal}/{collection}/{uid}.ics",
        name: "calendar-event",
        description: "A calendar entry serialized as iCalendar.",
        mime_type: "text/calendar",
    },
    ResourceTemplateSpec {
        uri_template: "loom://{workspace}/contacts/{principal}/{book}/{uid}.vcf",
        name: "contact-card",
        description: "A contact serialized as vCard.",
        mime_type: "text/vcard",
    },
    ResourceTemplateSpec {
        uri_template: "loom://{workspace}/mail/{principal}/{mailbox}/{uid}.eml",
        name: "mail-message",
        description: "A mail message serialized as RFC 5322 (.eml).",
        mime_type: "message/rfc822",
    },
    ResourceTemplateSpec {
        uri_template: "loom://{workspace}/studio/views/status/principal/{principal}",
        name: "studio-status",
        description: "Assistant session status view for one principal.",
        mime_type: "application/json",
    },
    ResourceTemplateSpec {
        uri_template: "loom://{workspace}/substrate/views/{view_id}.json",
        name: "substrate-view",
        description: "A deterministic substrate view definition.",
        mime_type: "application/json",
    },
    ResourceTemplateSpec {
        uri_template: "loom://{workspace}/substrate/refs/{target}.json",
        name: "substrate-refs",
        description: "Inbound typed references for one entity target.",
        mime_type: "application/json",
    },
];

/// Strip a trailing `.ext`, returning the stem.
pub(crate) fn strip_resource_ext<'a>(s: &'a str, ext: &str) -> Option<&'a str> {
    s.strip_suffix(ext)
}

/// Parse a `loom://<workspace>/<kind>/...` URI into a [`ResourceTarget`], or `None` if it does not
/// match a known shape.
pub fn parse_uri(uri: &str) -> Option<ResourceTarget> {
    if uri == "loom://capabilities.json" {
        return Some(ResourceTarget::Capabilities);
    }
    let rest = uri.strip_prefix("loom://")?;
    let (workspace, tail) = rest.split_once('/')?;
    if workspace.is_empty() {
        return None;
    }
    let workspace = workspace.to_string();
    if tail.is_empty() {
        return Some(ResourceTarget::Workspace { workspace });
    }
    let (kind, tail) = tail.split_once('/').unwrap_or((tail, ""));
    match kind {
        "files" => {
            if tail.is_empty() {
                return None;
            }
            Some(ResourceTarget::File {
                workspace,
                path: tail.to_string(),
            })
        }
        "cas" => {
            if tail.is_empty() {
                return None;
            }
            Some(ResourceTarget::Cas {
                workspace,
                digest: tail.to_string(),
            })
        }
        "calendar" | "contacts" | "mail" => {
            // <principal>/<container>/<uid>.<ext>
            let parts: Vec<&str> = tail.splitn(3, '/').collect();
            if parts.len() != 3 {
                return None;
            }
            let (principal, container, file) = (parts[0], parts[1], parts[2]);
            if principal.is_empty() || container.is_empty() {
                return None;
            }
            match kind {
                "calendar" => strip_resource_ext(file, ".ics")
                    .filter(|u| !u.is_empty())
                    .map(|uid| ResourceTarget::CalendarIcs {
                        workspace,
                        principal: principal.to_string(),
                        collection: container.to_string(),
                        uid: uid.to_string(),
                    }),
                "contacts" => strip_resource_ext(file, ".vcf")
                    .filter(|u| !u.is_empty())
                    .map(|uid| ResourceTarget::ContactsVcf {
                        workspace,
                        principal: principal.to_string(),
                        book: container.to_string(),
                        uid: uid.to_string(),
                    }),
                _ => strip_resource_ext(file, ".eml")
                    .filter(|u| !u.is_empty())
                    .map(|uid| ResourceTarget::MailEml {
                        workspace,
                        principal: principal.to_string(),
                        mailbox: container.to_string(),
                        uid: uid.to_string(),
                    }),
            }
        }
        "studio" => parse_studio_status_tail(tail).map(|principal| ResourceTarget::StudioStatus {
            workspace,
            principal: principal.to_string(),
        }),
        "substrate" => parse_substrate_tail(workspace, tail),
        _ => None,
    }
}

pub(crate) fn parse_studio_status_tail(tail: &str) -> Option<&str> {
    tail.strip_prefix("views/status/principal/")
        .filter(|principal| !principal.is_empty() && !principal.contains('/'))
}

pub(crate) fn parse_substrate_tail(workspace: String, tail: &str) -> Option<ResourceTarget> {
    let (kind, rest) = tail.split_once('/')?;
    match kind {
        "views" => strip_resource_ext(rest, ".json")
            .filter(|view_id| !view_id.is_empty() && !view_id.contains('/'))
            .map(|view_id| ResourceTarget::SubstrateView {
                workspace,
                view_id: view_id.to_string(),
            }),
        "refs" => strip_resource_ext(rest, ".json")
            .filter(|target| !target.is_empty() && !target.contains('/'))
            .map(|target| ResourceTarget::SubstrateRefs {
                workspace,
                target: target.to_string(),
            }),
        _ => None,
    }
}

/// Standard base64 (RFC 4648) encoder, for blob resource contents. Dependency-free.
pub fn base64_encode(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(T[(n >> 18 & 63) as usize] as char);
        out.push(T[(n >> 12 & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            T[(n >> 6 & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_each_shape() {
        assert_eq!(
            parse_uri("loom://capabilities.json"),
            Some(ResourceTarget::Capabilities)
        );
        assert_eq!(
            parse_uri("loom://app/"),
            Some(ResourceTarget::Workspace {
                workspace: "app".into(),
            })
        );
        assert_eq!(
            parse_uri("loom://app/files/dir/a.txt"),
            Some(ResourceTarget::File {
                workspace: "app".into(),
                path: "dir/a.txt".into()
            })
        );
        assert_eq!(
            parse_uri("loom://app/cas/blake3:abc"),
            Some(ResourceTarget::Cas {
                workspace: "app".into(),
                digest: "blake3:abc".into()
            })
        );
        assert_eq!(
            parse_uri("loom://app/calendar/alice/work/uid-1.ics"),
            Some(ResourceTarget::CalendarIcs {
                workspace: "app".into(),
                principal: "alice".into(),
                collection: "work".into(),
                uid: "uid-1".into(),
            })
        );
        assert_eq!(
            parse_uri("loom://app/contacts/alice/team/c1.vcf"),
            Some(ResourceTarget::ContactsVcf {
                workspace: "app".into(),
                principal: "alice".into(),
                book: "team".into(),
                uid: "c1".into(),
            })
        );
        assert_eq!(
            parse_uri("loom://app/mail/alice/inbox/m1.eml"),
            Some(ResourceTarget::MailEml {
                workspace: "app".into(),
                principal: "alice".into(),
                mailbox: "inbox".into(),
                uid: "m1".into(),
            })
        );
        assert_eq!(
            parse_uri("loom://app/studio/views/status/principal/alice"),
            Some(ResourceTarget::StudioStatus {
                workspace: "app".into(),
                principal: "alice".into(),
            })
        );
        assert_eq!(
            parse_uri("loom://app/substrate/views/status.json"),
            Some(ResourceTarget::SubstrateView {
                workspace: "app".into(),
                view_id: "status".into(),
            })
        );
        assert_eq!(
            parse_uri("loom://app/substrate/refs/ticket:42.json"),
            Some(ResourceTarget::SubstrateRefs {
                workspace: "app".into(),
                target: "ticket:42".into(),
            })
        );
    }

    #[test]
    fn rejects_bad_uris() {
        assert!(parse_uri("http://x/y").is_none());
        assert!(parse_uri("loom://app/unknown/x").is_none());
        assert!(parse_uri("loom://app/files/").is_none());
        assert!(parse_uri("loom://app/calendar/alice/work/uid-1.txt").is_none());
        assert!(parse_uri("loom://app/studio/views/status/principal/").is_none());
        assert!(parse_uri("loom://app/studio/views/status/principal/a/b").is_none());
        assert!(parse_uri("loom://app/substrate/views/.json").is_none());
        assert!(parse_uri("loom://app/substrate/views/a/b.json").is_none());
        assert!(parse_uri("loom://app/substrate/refs/a/b.json").is_none());
        assert!(parse_uri("loom:///files/x").is_none());
    }

    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"hello"), "aGVsbG8=");
    }

    #[test]
    fn templates_are_nonempty_and_well_formed() {
        assert!(!TEMPLATES.is_empty());
        for t in TEMPLATES {
            assert!(t.uri_template.starts_with("loom://{workspace}/"));
            assert!(!t.name.is_empty() && !t.description.is_empty());
        }
    }
}
