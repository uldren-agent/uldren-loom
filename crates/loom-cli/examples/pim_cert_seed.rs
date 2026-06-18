use std::path::{Path, PathBuf};

use loom_core::calendar::{CollectionMeta, Component};
use loom_core::contacts::BookMeta;
use loom_core::mail::MailboxMeta;
use loom_core::{
    AclRight, AclStore, AclSubject, Algo, FacetKind, IdentityStore, PrincipalKind, WorkspaceId,
    calendar, contacts, mail,
};
use loom_store::{FileStore, LocalOpenAuth, attach_local_auth, open_loom_unlocked, save_loom};

const ROOT_PRINCIPAL: WorkspaceId = WorkspaceId::from_bytes([0x01; 16]);
const FIXTURE_PRINCIPAL: WorkspaceId = WorkspaceId::from_bytes([0x42; 16]);
const CALENDAR_WORKSPACE: WorkspaceId = WorkspaceId::from_bytes([0x37; 16]);
const CONTACTS_WORKSPACE: WorkspaceId = WorkspaceId::from_bytes([0x38; 16]);
const MAIL_WORKSPACE: WorkspaceId = WorkspaceId::from_bytes([0x39; 16]);

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let store = required_arg(&mut args, "store path")?;
    let fixtures = required_arg(&mut args, "fixture directory")?;
    let account = required_arg(&mut args, "account")?;
    let password = required_arg(&mut args, "password")?;
    if args.next().is_some() {
        return Err("usage: pim_cert_seed <store> <fixture-dir> <account> <password>".into());
    }

    let store = PathBuf::from(store);
    let fixtures = PathBuf::from(fixtures);
    if store.exists() {
        return Err(format!("store already exists at {}", store.display()).into());
    }

    let fs = FileStore::create_with_profile(&store, Algo::Blake3)?;
    let mut identity = IdentityStore::new(ROOT_PRINCIPAL);
    identity.add_principal(FIXTURE_PRINCIPAL, account.clone(), PrincipalKind::Root)?;
    identity.set_passphrase(FIXTURE_PRINCIPAL, &password, b"pimcert1")?;
    fs.save_identity_store(&identity)?;

    let mut acl = AclStore::new();
    acl.allow(
        AclSubject::Principal(FIXTURE_PRINCIPAL),
        None,
        None,
        [
            AclRight::Admin,
            AclRight::Read,
            AclRight::Write,
            AclRight::Advance,
            AclRight::Merge,
            AclRight::Execute,
        ],
    )?;
    fs.save_acl_store(&acl)?;
    drop(fs);

    let auth = LocalOpenAuth {
        principal: Some(FIXTURE_PRINCIPAL),
        passphrase: Some(password),
        session_id: Some("pim-cert-seed".to_string()),
        ..LocalOpenAuth::default()
    };
    let mut loom = attach_local_auth(open_loom_unlocked(&store, None)?, &auth)?;

    let calendar_ns =
        loom.registry_mut()
            .create(FacetKind::Calendar, Some("calendar"), CALENDAR_WORKSPACE)?;
    let contacts_ns =
        loom.registry_mut()
            .create(FacetKind::Contacts, Some("contacts"), CONTACTS_WORKSPACE)?;
    let mail_ns = loom
        .registry_mut()
        .create(FacetKind::Mail, Some("mail"), MAIL_WORKSPACE)?;

    calendar::create_collection(
        &mut loom,
        calendar_ns,
        &account,
        "personal",
        &CollectionMeta {
            display_name: "Personal Calendar".to_string(),
            component_set: vec![Component::Event],
        },
    )?;
    contacts::create_book(
        &mut loom,
        contacts_ns,
        &account,
        "personal",
        &BookMeta {
            display_name: "Personal Contacts".to_string(),
        },
    )?;
    let mailboxes = [
        ("inbox", "Inbox"),
        ("Archive", "Archive"),
        ("Drafts", "Drafts"),
        ("Junk", "Junk"),
        ("Notes", "Notes"),
        ("Sent", "Sent"),
        ("Trash", "Trash"),
    ];
    for (mailbox, display_name) in mailboxes {
        mail::create_mailbox(
            &mut loom,
            mail_ns,
            &account,
            mailbox,
            &MailboxMeta {
                display_name: display_name.to_string(),
            },
        )?;
        mail::subscribe_imap_mailbox(&mut loom, mail_ns, &account, mailbox)?;
    }

    for idx in 1..=3 {
        let ics = read_text(&fixtures, &format!("calendar/event-{idx}.ics"))?;
        calendar::put_ics(&mut loom, calendar_ns, &account, "personal", &ics)?;
    }
    for idx in 1..=3 {
        let vcf = read_text(&fixtures, &format!("contacts/contact-{idx}.vcf"))?;
        contacts::put_vcard(&mut loom, contacts_ns, &account, "personal", &vcf)?;
    }
    for idx in 1..=3 {
        let raw = std::fs::read(fixtures.join(format!("mail/message-{idx}.eml")))?;
        mail::ingest_message(
            &mut loom,
            mail_ns,
            &account,
            "inbox",
            &format!("msg-{idx}"),
            &raw,
        )?;
    }

    loom.commit(calendar_ns, "pim-cert", "seed calendar", 0)?;
    loom.commit(contacts_ns, "pim-cert", "seed contacts", 0)?;
    loom.commit(mail_ns, "pim-cert", "seed mail", 0)?;
    save_loom(&mut loom)?;
    println!("{FIXTURE_PRINCIPAL}");
    Ok(())
}

fn required_arg(
    args: &mut impl Iterator<Item = String>,
    name: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    args.next()
        .ok_or_else(|| format!("missing required argument: {name}").into())
}

fn read_text(root: &Path, relative: &str) -> Result<String, Box<dyn std::error::Error>> {
    Ok(std::fs::read_to_string(root.join(relative))?)
}
