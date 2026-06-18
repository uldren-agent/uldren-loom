use loom_core::calendar::{self, CalendarEntry, CollectionMeta};
use loom_core::contacts::{self, BookMeta, ContactEntry};
use loom_core::mail::{self, MailMessage, MailboxMeta};
use loom_core::{Digest, WorkspaceId};

use loom_hosted_core::{HostedAuth, HostedKernel, HostedOutcome, hosted_outcome};

pub struct HostedPimAdapter<'a> {
    kernel: &'a HostedKernel,
}

pub trait HostedPimKernelExt {
    fn pim(&self) -> HostedPimAdapter<'_>;
}

impl HostedPimKernelExt for HostedKernel {
    fn pim(&self) -> HostedPimAdapter<'_> {
        HostedPimAdapter { kernel: self }
    }
}

impl HostedPimAdapter<'_> {
    pub fn calendar_create_collection(
        &self,
        auth: &HostedAuth,
        ns: WorkspaceId,
        principal: &str,
        collection: &str,
        meta: &CollectionMeta,
    ) -> HostedOutcome<()> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            calendar::create_collection(loom, ns, principal, collection, meta)
        }))
    }

    pub fn calendar_get_entry(
        &self,
        auth: &HostedAuth,
        ns: WorkspaceId,
        principal: &str,
        collection: &str,
        uid: &str,
    ) -> HostedOutcome<Option<CalendarEntry>> {
        hosted_outcome(self.kernel.read(auth, |loom| {
            calendar::get_entry(loom, ns, principal, collection, uid)
        }))
    }

    pub fn calendar_put_entry(
        &self,
        auth: &HostedAuth,
        ns: WorkspaceId,
        principal: &str,
        collection: &str,
        entry: &CalendarEntry,
    ) -> HostedOutcome<Digest> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            calendar::put_entry(loom, ns, principal, collection, entry)
        }))
    }

    pub fn contacts_create_book(
        &self,
        auth: &HostedAuth,
        ns: WorkspaceId,
        principal: &str,
        book: &str,
        meta: &BookMeta,
    ) -> HostedOutcome<()> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            contacts::create_book(loom, ns, principal, book, meta)
        }))
    }

    pub fn contacts_get_entry(
        &self,
        auth: &HostedAuth,
        ns: WorkspaceId,
        principal: &str,
        book: &str,
        uid: &str,
    ) -> HostedOutcome<Option<ContactEntry>> {
        hosted_outcome(self.kernel.read(auth, |loom| {
            contacts::get_entry(loom, ns, principal, book, uid)
        }))
    }

    pub fn contacts_put_entry(
        &self,
        auth: &HostedAuth,
        ns: WorkspaceId,
        principal: &str,
        book: &str,
        entry: &ContactEntry,
    ) -> HostedOutcome<Digest> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            contacts::put_entry(loom, ns, principal, book, entry)
        }))
    }

    pub fn mail_create_mailbox(
        &self,
        auth: &HostedAuth,
        ns: WorkspaceId,
        principal: &str,
        mailbox: &str,
        meta: &MailboxMeta,
    ) -> HostedOutcome<()> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            mail::create_mailbox(loom, ns, principal, mailbox, meta)
        }))
    }

    pub fn mail_get_message(
        &self,
        auth: &HostedAuth,
        ns: WorkspaceId,
        principal: &str,
        mailbox: &str,
        uid: &str,
    ) -> HostedOutcome<Option<MailMessage>> {
        hosted_outcome(self.kernel.read(auth, |loom| {
            mail::get_message(loom, ns, principal, mailbox, uid)
        }))
    }

    pub fn mail_ingest_message(
        &self,
        auth: &HostedAuth,
        ns: WorkspaceId,
        principal: &str,
        mailbox: &str,
        uid: &str,
        raw: &[u8],
    ) -> HostedOutcome<Digest> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            mail::ingest_message(loom, ns, principal, mailbox, uid, raw)
        }))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use loom_core::Code;
    use loom_core::calendar::{CalendarEntry, CollectionMeta, Component};
    use loom_core::contacts::{BookMeta, ContactEntry};
    use loom_core::mail::MailboxMeta;

    use super::HostedPimKernelExt;
    use loom_hosted_core::test_support::{init, nid, temp_path};
    use loom_hosted_core::{HostedAuth, HostedKernel};

    #[test]
    fn hosted_calendar_attaches_auth_and_pep() {
        let path = temp_path("calendar");
        let ns = init(&path, None);
        let kernel = HostedKernel::new(&path);
        let pim = kernel.pim();
        let auth = HostedAuth::passphrase(nid(1), "root-pass", "pim-1");
        let meta = CollectionMeta {
            display_name: "Work".to_string(),
            component_set: vec![Component::Event],
        };
        pim.calendar_create_collection(&auth, ns, "root", "work", &meta)
            .unwrap();
        let entry = CalendarEntry::event("event-1", "Meeting", "20260101T120000Z");
        pim.calendar_put_entry(&auth, ns, "root", "work", &entry)
            .unwrap();
        assert_eq!(
            pim.calendar_get_entry(&auth, ns, "root", "work", "event-1")
                .unwrap()
                .unwrap()
                .summary,
            "Meeting"
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn hosted_pim_denies_unauthenticated_or_ungranted_principals() {
        let path = temp_path("pim-denied");
        let user = nid(2);
        let ns = init(&path, Some(user));
        let kernel = HostedKernel::new(&path);
        let pim = kernel.pim();
        let meta = CollectionMeta {
            display_name: "Work".to_string(),
            component_set: vec![Component::Event],
        };
        let missing = pim
            .calendar_create_collection(&HostedAuth::unauthenticated(), ns, "root", "work", &meta)
            .unwrap_err();
        assert_eq!(missing.code, Code::AuthenticationFailed);

        let alice = HostedAuth::passphrase(user, "alice-pass", "pim-alice");
        let denied = pim
            .calendar_create_collection(&alice, ns, "root", "work", &meta)
            .unwrap_err();
        assert_eq!(denied.code, Code::PermissionDenied);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn hosted_contacts_and_mail_attach_auth_and_pep() {
        let path = temp_path("contacts-mail");
        let ns = init(&path, None);
        let kernel = HostedKernel::new(&path);
        let pim = kernel.pim();
        let auth = HostedAuth::passphrase(nid(1), "root-pass", "pim-1");

        pim.contacts_create_book(
            &auth,
            ns,
            "root",
            "people",
            &BookMeta {
                display_name: "People".to_string(),
            },
        )
        .unwrap();
        pim.contacts_put_entry(
            &auth,
            ns,
            "root",
            "people",
            &ContactEntry::new("contact-1", "Alice Example"),
        )
        .unwrap();
        assert_eq!(
            pim.contacts_get_entry(&auth, ns, "root", "people", "contact-1")
                .unwrap()
                .unwrap()
                .full_name,
            "Alice Example"
        );

        pim.mail_create_mailbox(
            &auth,
            ns,
            "root",
            "inbox",
            &MailboxMeta {
                display_name: "Inbox".to_string(),
            },
        )
        .unwrap();
        pim.mail_ingest_message(
            &auth,
            ns,
            "root",
            "inbox",
            "m1",
            b"From: a@example.com\r\nTo: b@example.com\r\nSubject: Hi\r\n\r\nBody",
        )
        .unwrap();
        assert_eq!(
            pim.mail_get_message(&auth, ns, "root", "inbox", "m1")
                .unwrap()
                .unwrap()
                .subject,
            "Hi"
        );
        fs::remove_file(path).unwrap();
    }
}
