//! Reusable PIM record models and wire projections for Loom calendar, contacts, and mail facets.

mod cbor;

pub mod calendar;
pub mod contacts;
pub mod mail;

pub use calendar::{
    CalendarEntry, CollectionMeta, Component, ComponentField, DateTime, IcalDate, IcalMonth,
    IcalTime,
};
pub use contacts::{BookMeta, ContactEntry, TypedValue, VcardProperty};
pub use mail::{MailMessage, MailboxMeta};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendar_record_codecs_and_ics_projection_round_trip() {
        let mut entry = CalendarEntry::event("u1", "Standup", "20240101T090000");
        entry.rrule = Some("FREQ=WEEKLY;BYDAY=MO".to_string());
        entry.exdate = vec!["20240115T090000".to_string()];
        entry.extra = vec![("X-ROOM".to_string(), "blue".to_string())];

        let decoded = CalendarEntry::decode(&entry.encode()).unwrap();
        assert_eq!(decoded, entry);

        let ics = entry.to_ics();
        let from_ics = CalendarEntry::from_ics(&ics).unwrap();
        assert_eq!(from_ics.uid, "u1");
        assert_eq!(from_ics.summary, "Standup");
        assert_eq!(from_ics.rrule.as_deref(), Some("FREQ=WEEKLY;BYDAY=MO"));
    }

    #[test]
    fn calendar_ics_projection_covers_rfc5545_bounded_profile() {
        let mut entry = CalendarEntry::event(
            "u5545",
            "Lunch, roadmap; line\\break\nnext",
            "20240101T090000",
        );
        entry.dtend = Some("20240101T100000".to_string());
        entry.tzid = Some("America/New_York".to_string());
        entry.rrule = Some("FREQ=WEEKLY;COUNT=3;BYDAY=MO".to_string());
        entry.rdate = vec!["20240122T090000".to_string()];
        entry.exdate = vec!["20240115T090000".to_string()];
        entry.status = Some("CONFIRMED".to_string());
        entry.extra = vec![
            (
                "DESCRIPTION".to_string(),
                "Discuss scope, budget; and rollout".to_string(),
            ),
            ("SEQUENCE".to_string(), "7".to_string()),
            ("X-LOOM-ROOM".to_string(), "C-4".to_string()),
        ];

        let ics = entry.to_ics();
        assert!(ics.contains("BEGIN:VCALENDAR\r\n"));
        assert!(ics.contains("VERSION:2.0\r\n"));
        assert!(ics.contains("BEGIN:VEVENT\r\n"));
        assert!(ics.contains("UID:u5545\r\n"));
        assert!(ics.contains("SUMMARY:Lunch\\, roadmap\\; line\\\\break\\nnext\r\n"));
        assert!(ics.contains("DTSTART;TZID=America/New_York:20240101T090000\r\n"));
        assert!(ics.contains("DTEND;TZID=America/New_York:20240101T100000\r\n"));
        assert!(ics.contains("RRULE:FREQ=WEEKLY;COUNT=3;BYDAY=MO\r\n"));
        assert!(ics.contains("RDATE:20240122T090000\r\n"));
        assert!(ics.contains("EXDATE:20240115T090000\r\n"));
        assert!(ics.contains("STATUS:CONFIRMED\r\n"));
        assert!(ics.contains("DESCRIPTION:Discuss scope\\, budget\\; and rollout\r\n"));
        assert!(ics.contains("SEQUENCE:7\r\n"));
        assert!(ics.contains("X-LOOM-ROOM:C-4\r\n"));
        assert_eq!(CalendarEntry::from_ics(&ics).unwrap(), entry);
    }

    #[test]
    fn calendar_ics_projection_folds_and_unfolds_long_lines() {
        let mut entry = CalendarEntry::event("u-fold", "x".repeat(200), "20240101T090000");
        entry.component = ComponentField(Component::Todo);

        let ics = entry.to_ics();
        assert!(ics.contains("BEGIN:VTODO\r\n"));
        assert!(ics.contains("\r\n "));
        for line in ics.trim_end_matches("\r\n").split("\r\n") {
            assert!(
                line.len() <= 75,
                "iCalendar content line exceeds 75 octets: {line:?}"
            );
        }
        assert_eq!(CalendarEntry::from_ics(&ics).unwrap(), entry);
    }

    #[test]
    fn calendar_ics_parser_rejects_missing_required_profile_fields() {
        let missing_uid = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nSUMMARY:Missing UID\r\nDTSTART:20240101T090000\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let missing_dtstart = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:u-missing\r\nSUMMARY:Missing DTSTART\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

        assert!(CalendarEntry::from_ics(missing_uid).is_err());
        assert!(CalendarEntry::from_ics(missing_dtstart).is_err());
    }

    #[test]
    fn calendar_ics_preserves_rfc7986_component_property_values_only() {
        let ics = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nNAME:Team Calendar\r\nCOLOR:blue\r\nBEGIN:VEVENT\r\nUID:u7986\r\nSUMMARY:Planning\r\nDTSTART:20240101T090000\r\nCOLOR:turquoise\r\nIMAGE;VALUE=URI:https://example.test/image.png\r\nCONFERENCE;VALUE=URI;FEATURE=AUDIO:https://meet.example.test/room\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

        let entry = CalendarEntry::from_ics(ics).unwrap();
        assert_eq!(entry.uid, "u7986");
        assert!(
            entry
                .extra
                .contains(&("COLOR".to_string(), "turquoise".to_string()))
        );
        assert!(entry.extra.contains(&(
            "IMAGE".to_string(),
            "https://example.test/image.png".to_string()
        )));
        assert!(entry.extra.contains(&(
            "CONFERENCE".to_string(),
            "https://meet.example.test/room".to_string()
        )));
        assert!(!entry.extra.iter().any(|(name, _)| name == "NAME"));

        let round_trip = CalendarEntry::from_ics(&entry.to_ics()).unwrap();
        let mut expected = entry.extra.clone();
        let mut actual = round_trip.extra;
        expected.sort();
        actual.sort();
        assert_eq!(actual, expected);
    }

    #[test]
    fn contacts_record_codecs_and_vcard_projection_round_trip() {
        let mut entry = ContactEntry::new("c1", "Ada Lovelace");
        entry.emails = vec![TypedValue::typed("ada@example.test", "work")];
        entry.extra = vec![("X-CUSTOM".to_string(), "yes".to_string())];

        let decoded = ContactEntry::decode(&entry.encode()).unwrap();
        assert_eq!(decoded, entry);

        let vcard = entry.to_vcard();
        let from_vcard = ContactEntry::from_vcard(&vcard).unwrap();
        assert_eq!(from_vcard.uid, "c1");
        assert_eq!(from_vcard.full_name, "Ada Lovelace");
        assert_eq!(from_vcard.emails[0].value, "ada@example.test");
    }

    #[test]
    fn contacts_vcard_projection_covers_rfc6350_bounded_profile() {
        let mut entry = ContactEntry::new("c6350", "Ada, Lovelace; Countess\nLine");
        entry.n = Some("Lovelace;Ada;;;".to_string());
        entry.emails = vec![TypedValue::typed("ada@example.test", "work")];
        entry.tels = vec![TypedValue::typed("+15550123", "cell")];
        entry.org = Some("Uldren Labs".to_string());
        entry.title = Some("Principal ".repeat(16));
        entry.extra = vec![(
            "X-NOTE".to_string(),
            "comma, semicolon; newline\nnext".to_string(),
        )];

        let vcard = entry.to_vcard();
        assert!(vcard.contains("BEGIN:VCARD"));
        assert!(vcard.contains("VERSION:4.0"));
        assert!(vcard.contains("UID:c6350"));
        assert!(vcard.contains("EMAIL"));
        assert!(vcard.contains("TYPE=work"));
        assert!(vcard.contains("TEL"));
        assert!(vcard.contains("TYPE=cell"));
        assert!(vcard.contains("\r\n ") || vcard.contains("\n "));

        let from_vcard = ContactEntry::from_vcard(&vcard).unwrap();
        assert_eq!(from_vcard.uid, "c6350");
        assert_eq!(from_vcard.full_name, "Ada, Lovelace; Countess\nLine");
        assert_eq!(from_vcard.n.as_deref(), Some("Lovelace;Ada;;;"));
        assert_eq!(
            from_vcard.emails[0],
            TypedValue::typed("ada@example.test", "work")
        );
        assert_eq!(from_vcard.tels[0], TypedValue::typed("+15550123", "cell"));
        assert_eq!(from_vcard.org.as_deref(), Some("Uldren Labs"));
        assert_eq!(from_vcard.title, entry.title);
        assert!(from_vcard.extra.iter().any(|(name, value)| {
            name == "X-NOTE" && value == "comma, semicolon; newline\nnext"
        }));
    }

    #[test]
    fn contacts_vcard3_projection_covers_carddav_mandatory_profile() {
        let mut entry = ContactEntry::new("c2426", "Ada, Lovelace; Countess\nLine");
        entry.n = Some("Lovelace;Ada;;;".to_string());
        entry.emails = vec![TypedValue::typed("ada@example.test", "work")];
        entry.tels = vec![TypedValue::typed("+15550123", "cell")];
        entry.org = Some("Uldren Labs".to_string());
        entry.title = Some("Principal".to_string());

        let vcard = entry.to_vcard3();
        assert!(vcard.contains("BEGIN:VCARD\r\n"));
        assert!(vcard.contains("VERSION:3.0\r\n"));
        assert!(vcard.contains("FN:Ada\\, Lovelace\\; Countess\\nLine\r\n"));
        assert!(vcard.contains("N:Lovelace;Ada;;;\r\n"));
        assert!(vcard.contains("EMAIL;TYPE=INTERNET;TYPE=WORK:ada@example.test\r\n"));
        assert!(vcard.contains("TEL;TYPE=CELL:+15550123\r\n"));
        assert!(vcard.contains("UID:c2426\r\n"));

        let from_vcard = ContactEntry::from_vcard(&vcard).unwrap();
        assert_eq!(from_vcard.uid, "c2426");
        assert_eq!(from_vcard.full_name, "Ada, Lovelace; Countess\nLine");
        assert_eq!(from_vcard.n.as_deref(), Some("Lovelace;Ada;;;"));
    }

    #[test]
    fn contacts_vcard3_parser_covers_apple_carddav_write_profile() {
        let raw = "BEGIN:VCARD\r\nVERSION:3.0\r\nN:Lovelace;Ada;;Countess;\r\nFN:Ada Lovelace\r\nEMAIL;TYPE=INTERNET;TYPE=WORK:ada@example.test\r\nTEL;TYPE=CELL:+15550123\r\nitem1.X-ABLabel:_$!<Work>!$_\r\nADR;TYPE=HOME:;;1 Infinite Loop;Cupertino;CA;95014;USA\r\nUID:c-apple\r\nEND:VCARD\r\n";

        let entry = ContactEntry::from_vcard(raw).unwrap();
        assert_eq!(entry.uid, "c-apple");
        assert_eq!(entry.full_name, "Ada Lovelace");
        assert_eq!(entry.n.as_deref(), Some("Lovelace;Ada;;Countess;"));
        assert_eq!(
            entry.emails,
            vec![TypedValue::typed("ada@example.test", "work")]
        );
        assert_eq!(entry.tels, vec![TypedValue::typed("+15550123", "cell")]);
        assert!(entry.vcard3_properties.iter().any(|property| {
            property.group.as_deref() == Some("item1")
                && property.name == "X-ABLABEL"
                && property.value == "_$!<Work>!$_"
        }));
        assert!(
            entry
                .vcard3_properties
                .iter()
                .any(|property| property.name == "ADR")
        );

        let projected = entry.to_vcard3();
        assert!(projected.contains("item1.X-ABLABEL:_$!<Work>!$_\r\n"));
        assert!(projected.contains("ADR;TYPE=HOME:;;1 Infinite Loop;Cupertino;CA;95014;USA\r\n"));
    }

    #[test]
    fn contacts_vcard3_parser_preserves_rfc2426_registered_properties() {
        let raw = "BEGIN:VCARD\r\nVERSION:3.0\r\nPROFILE:VCARD\r\nSOURCE:http://example.test/source.vcf\r\nNAME:Directory Entry\r\nNICKNAME:Ada,Enchantress\r\nPHOTO;VALUE=URI:http://example.test/photo.jpg\r\nBDAY:18151210\r\nADR;TYPE=HOME:;;1 Loop;Cupertino;CA;95014;USA\r\nLABEL;TYPE=HOME:1 Loop\\nCupertino\\, CA\r\nMAILER:Example Mailer\r\nTZ:+00:00\r\nGEO:37.386013;-122.082932\r\nROLE:Mathematician\r\nLOGO;VALUE=URI:http://example.test/logo.png\r\nAGENT:BEGIN:VCARD\\nFN:Assistant\\nN:Assistant;;;;\\nVERSION:3.0\\nEND:VCARD\\n\r\nCATEGORIES:math,computing\r\nNOTE:comma\\, semicolon\\; newline\\nnext\r\nPRODID:-//Uldren//Loom//EN\r\nREV:20240101T000000Z\r\nSORT-STRING:Lovelace\r\nSOUND;VALUE=URI:http://example.test/sound.wav\r\nURL:http://example.test\r\nCLASS:PUBLIC\r\nKEY;VALUE=URI:http://example.test/key.asc\r\nN:Lovelace;Ada;;Countess;\r\nFN:Ada Lovelace\r\nTITLE:Countess\r\nORG:Uldren Labs;Research\r\nTEL;TYPE=WORK:+15550123\r\nEMAIL;TYPE=INTERNET;TYPE=WORK:ada@example.test\r\nUID:c-rfc2426\r\nEND:VCARD\r\n";

        let entry = ContactEntry::from_vcard(raw).unwrap();
        let preserved = entry
            .vcard3_properties
            .iter()
            .map(|property| property.name.as_str())
            .collect::<Vec<_>>();
        for name in [
            "PROFILE",
            "SOURCE",
            "NAME",
            "NICKNAME",
            "PHOTO",
            "BDAY",
            "ADR",
            "LABEL",
            "MAILER",
            "TZ",
            "GEO",
            "ROLE",
            "LOGO",
            "AGENT",
            "CATEGORIES",
            "NOTE",
            "PRODID",
            "REV",
            "SORT-STRING",
            "SOUND",
            "URL",
            "CLASS",
            "KEY",
        ] {
            assert!(preserved.contains(&name), "{name}");
        }
        assert_eq!(entry.title.as_deref(), Some("Countess"));
        assert_eq!(entry.org.as_deref(), Some("Uldren Labs;Research"));

        let projected = entry.to_vcard3();
        assert!(projected.contains("PHOTO;VALUE=URI:http://example.test/photo.jpg\r\n"));
        assert!(projected.contains("KEY;VALUE=URI:http://example.test/key.asc\r\n"));
        assert!(projected.contains(
            "AGENT:BEGIN:VCARD\\nFN:Assistant\\nN:Assistant;;;;\\nVERSION:3.0\\nEND:VCARD\\n\r\n"
        ));
    }

    #[test]
    fn contacts_vcard_parser_rejects_missing_required_profile_fields() {
        let missing_uid = "BEGIN:VCARD\r\nVERSION:4.0\r\nFN:Ada Lovelace\r\nEND:VCARD\r\n";
        let missing_fn = "BEGIN:VCARD\r\nVERSION:4.0\r\nUID:c6350\r\nEND:VCARD\r\n";

        assert!(ContactEntry::from_vcard(missing_uid).is_err());
        assert!(ContactEntry::from_vcard(missing_fn).is_err());
    }

    #[test]
    fn contacts_vcard3_parser_derives_missing_structured_name_for_compatibility() {
        let raw = "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:c2426\r\nFN:Ada Lovelace\r\nEND:VCARD\r\n";

        let entry = ContactEntry::from_vcard(raw).unwrap();
        assert_eq!(entry.n.as_deref(), Some("Ada Lovelace;;;;"));
        assert!(entry.to_vcard3().contains("N:Ada Lovelace;;;;\r\n"));
    }

    #[test]
    fn contacts_vcard_rejects_rfc6474_place_death_properties() {
        for property in [
            "BIRTHPLACE:London",
            "DEATHPLACE:Aboard the Titanic\\, near Newfoundland",
            "DEATHDATE:19960415",
        ] {
            let raw = format!(
                "BEGIN:VCARD\r\nVERSION:4.0\r\nUID:c6474\r\nFN:Ada Lovelace\r\n{property}\r\nEND:VCARD\r\n"
            );
            assert!(ContactEntry::from_vcard(&raw).is_err(), "{property}");
        }
    }

    #[test]
    fn contacts_vcard_caret_parameter_encoding_remains_target() {
        let raw = "BEGIN:VCARD\r\nVERSION:4.0\r\nUID:c6868\r\nFN:Ada Lovelace\r\nEMAIL;TYPE=x-work^^quoted:ada@example.test\r\nEND:VCARD\r\n";

        if let Ok(entry) = ContactEntry::from_vcard(raw) {
            assert_ne!(entry.emails[0].kind.as_deref(), Some("x-work^quoted"));
        }
    }

    #[test]
    fn mail_record_codecs_and_rfc5322_projection_round_trip() {
        let raw = b"From: sender@example.test\r\nTo: receiver@example.test\r\nSubject: Hello\r\nMessage-ID: <m1@example.test>\r\n\r\nbody";
        let message = MailMessage::from_rfc5322("m1", "0123", raw).unwrap();
        assert_eq!(message.uid, "m1");
        assert_eq!(message.body, "0123");
        assert_eq!(message.from, "sender@example.test");
        assert_eq!(message.subject, "Hello");

        let decoded = MailMessage::decode(&message.encode()).unwrap();
        assert_eq!(decoded, message);
    }

    #[test]
    fn mail_rfc5322_projection_covers_bounded_profile() {
        let raw = b"From: Sender <sender@example.test>\r\nTo: receiver@example.test,\r\n team@example.test\r\nSubject: Hello\r\n\tFolded\r\nDate: Mon, 1 Jan 2024 09:00:00 +0000\r\nMessage-ID: <m-fold@example.test>\r\nX-Trace: one\r\n two\r\n\r\nbody\r\n";

        let message = MailMessage::from_rfc5322("m-fold", "abcd", raw).unwrap();
        assert_eq!(message.from, "sender@example.test");
        assert_eq!(
            message.to,
            vec![
                "receiver@example.test".to_string(),
                "team@example.test".to_string()
            ]
        );
        assert_eq!(message.subject, "Hello Folded");
        assert_eq!(message.message_id.as_deref(), Some("m-fold@example.test"));
        assert!(!message.date.is_empty());
        assert_eq!(message.size, raw.len() as u64);
        assert!(
            message
                .headers
                .iter()
                .any(|(name, value)| name == "Subject" && value == "Hello Folded")
        );
        assert!(
            message
                .headers
                .iter()
                .any(|(name, value)| name == "X-Trace" && value == "one two")
        );
    }
}
