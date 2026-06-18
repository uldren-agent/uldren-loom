use loom_core::calendar::{CalendarEntry, CollectionMeta};
use loom_core::contacts::{BookMeta, ContactEntry};
use loom_core::mail::{MailMessage, MailboxMeta};
use loom_core::{Digest, WorkspaceId};
use loom_hosted_pim::HostedPimKernelExt;

use crate::{HostedAuth, HostedKernel, HostedOutcome};

pub struct HostedPimAdapter<'a> {
    inner: loom_hosted_pim::HostedPimAdapter<'a>,
}

impl HostedKernel {
    pub fn pim(&self) -> HostedPimAdapter<'_> {
        HostedPimAdapter {
            inner: self.as_core().pim(),
        }
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
        self.inner
            .calendar_create_collection(auth, ns, principal, collection, meta)
    }

    pub fn calendar_get_entry(
        &self,
        auth: &HostedAuth,
        ns: WorkspaceId,
        principal: &str,
        collection: &str,
        uid: &str,
    ) -> HostedOutcome<Option<CalendarEntry>> {
        self.inner
            .calendar_get_entry(auth, ns, principal, collection, uid)
    }

    pub fn calendar_put_entry(
        &self,
        auth: &HostedAuth,
        ns: WorkspaceId,
        principal: &str,
        collection: &str,
        entry: &CalendarEntry,
    ) -> HostedOutcome<Digest> {
        self.inner
            .calendar_put_entry(auth, ns, principal, collection, entry)
    }

    pub fn contacts_create_book(
        &self,
        auth: &HostedAuth,
        ns: WorkspaceId,
        principal: &str,
        book: &str,
        meta: &BookMeta,
    ) -> HostedOutcome<()> {
        self.inner
            .contacts_create_book(auth, ns, principal, book, meta)
    }

    pub fn contacts_get_entry(
        &self,
        auth: &HostedAuth,
        ns: WorkspaceId,
        principal: &str,
        book: &str,
        uid: &str,
    ) -> HostedOutcome<Option<ContactEntry>> {
        self.inner
            .contacts_get_entry(auth, ns, principal, book, uid)
    }

    pub fn contacts_put_entry(
        &self,
        auth: &HostedAuth,
        ns: WorkspaceId,
        principal: &str,
        book: &str,
        entry: &ContactEntry,
    ) -> HostedOutcome<Digest> {
        self.inner
            .contacts_put_entry(auth, ns, principal, book, entry)
    }

    pub fn mail_create_mailbox(
        &self,
        auth: &HostedAuth,
        ns: WorkspaceId,
        principal: &str,
        mailbox: &str,
        meta: &MailboxMeta,
    ) -> HostedOutcome<()> {
        self.inner
            .mail_create_mailbox(auth, ns, principal, mailbox, meta)
    }

    pub fn mail_get_message(
        &self,
        auth: &HostedAuth,
        ns: WorkspaceId,
        principal: &str,
        mailbox: &str,
        uid: &str,
    ) -> HostedOutcome<Option<MailMessage>> {
        self.inner
            .mail_get_message(auth, ns, principal, mailbox, uid)
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
        self.inner
            .mail_ingest_message(auth, ns, principal, mailbox, uid, raw)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use loom_core::calendar::{CalendarEntry, CollectionMeta, Component};

    use crate::test_support::{init, nid, temp_path};
    use crate::{HostedAuth, HostedKernel};

    #[test]
    fn hosted_pim_adapter_delegates_to_pim_crate_boundary() {
        let path = temp_path("pim-compat");
        let ns = init(&path, None);
        let kernel = HostedKernel::new(&path);
        let pim = kernel.pim();
        let auth = HostedAuth::passphrase(nid(1), "root-pass", "pim-compat");
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
}
