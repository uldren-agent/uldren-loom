#[cfg(feature = "http")]
pub mod dav;
#[cfg(feature = "http")]
pub mod imap;
#[cfg(feature = "http")]
pub mod jmap;
#[cfg(feature = "http")]
pub mod pim;
#[cfg(feature = "http")]
pub mod smtp;

#[cfg(feature = "http")]
pub use dav::{
    HostedDavWorkspaces, caldav_router, caldav_router_with_limit, caldav_router_with_policy,
    carddav_router, carddav_router_with_limit, carddav_router_with_policy, dav_router_with_policy,
    serve_caldav_with_limits, serve_carddav_with_limits, serve_dav_with_limits,
};
#[cfg(all(feature = "http", feature = "tls"))]
pub use dav::{
    serve_caldav_tls_with_limits, serve_carddav_tls_with_limits, serve_dav_tls_with_limits,
};
#[cfg(feature = "http")]
pub use imap::serve_mail_imap;
#[cfg(all(feature = "http", feature = "tls"))]
pub use imap::serve_mail_imap_tls;
#[cfg(feature = "http")]
pub use jmap::{
    mail_jmap_router, mail_jmap_router_with_limit, mail_jmap_router_with_policy,
    serve_mail_jmap_with_limits,
};
#[cfg(feature = "http")]
pub use pim::{HostedPimAdapter, HostedPimKernelExt};
#[cfg(feature = "http")]
pub use smtp::serve_mail_smtp;
#[cfg(all(feature = "http", feature = "tls"))]
pub use smtp::{serve_mail_smtp_starttls, serve_mail_smtp_tls};
