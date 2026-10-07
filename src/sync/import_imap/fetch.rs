/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use crate::imap::response::{Untagged, Value};

#[derive(Debug, Default, Clone)]
pub struct FetchAttrs {
    pub uid: Option<u32>,
    pub gmail_msgid: Option<u64>,
    pub flags: Vec<String>,
    pub internaldate: Option<String>,
    pub size: Option<u64>,
    pub body: Option<Vec<u8>>,
}

pub fn extract(u: &Untagged) -> Option<FetchAttrs> {
    let Untagged::Fetch { items, .. } = u else {
        return None;
    };
    let mut out = FetchAttrs::default();
    for (name, value) in items {
        match name.as_str() {
            "UID" => {
                if let Some(n) = value.as_number() {
                    out.uid = Some(n as u32);
                }
            }
            "X-GM-MSGID" => {
                if let Some(n) = value.as_number() {
                    out.gmail_msgid = Some(n);
                }
            }
            "FLAGS" => {
                if let Value::List(items) = value {
                    out.flags = items
                        .iter()
                        .filter_map(|v| match v {
                            Value::Atom(s) | Value::Str(s) => Some(s.clone()),
                            _ => None,
                        })
                        .collect();
                }
            }
            "INTERNALDATE" => {
                if let Some(s) = value.as_str() {
                    out.internaldate = Some(s.to_owned());
                }
            }
            "RFC822.SIZE" => {
                if let Some(n) = value.as_number() {
                    out.size = Some(n);
                }
            }
            n if n == "BODY[]" || n == "RFC822" => {
                out.body = match value {
                    Value::Bytes(b) => Some(b.clone()),
                    Value::Str(s) => Some(s.clone().into_bytes()),
                    _ => None,
                };
            }
            _ => {}
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn parse_fetch(input: &[u8]) -> FetchAttrs {
        let resp = crate::imap::response::parse_response(&mut Cursor::new(input)).unwrap();
        match resp {
            crate::imap::response::Response::Untagged(u) => extract(&u).expect("fetch attrs"),
            _ => panic!("expected untagged"),
        }
    }

    #[test]
    fn extracts_uid_flags_size_internaldate() {
        let f = parse_fetch(
            b"* 1 FETCH (UID 42 FLAGS (\\Seen) INTERNALDATE \"01-Jan-2024 12:00:00 +0000\" RFC822.SIZE 4242)\r\n",
        );
        assert_eq!(f.uid, Some(42));
        assert_eq!(f.gmail_msgid, None);
        assert_eq!(f.flags, vec!["\\Seen"]);
        assert_eq!(
            f.internaldate.as_deref(),
            Some("01-Jan-2024 12:00:00 +0000")
        );
        assert_eq!(f.size, Some(4242));
        assert!(f.body.is_none());
    }

    #[test]
    fn extracts_gmail_message_id() {
        let f =
            parse_fetch(b"* 1 FETCH (UID 42 X-GM-MSGID 1794908265753564076 FLAGS (\\Seen))\r\n");
        assert_eq!(f.uid, Some(42));
        assert_eq!(f.gmail_msgid, Some(1794908265753564076));
    }

    #[test]
    fn extracts_full_unsigned_gmail_message_id() {
        let f = parse_fetch(b"* 1 FETCH (UID 42 X-GM-MSGID 18446744073709551615)\r\n");
        assert_eq!(f.gmail_msgid, Some(u64::MAX));
    }

    #[test]
    fn extracts_body_literal_as_bytes() {
        let input = b"* 1 FETCH (UID 5 BODY[] {11}\r\nHello world)\r\n";
        let f = parse_fetch(input);
        assert_eq!(f.body.as_deref(), Some(&b"Hello world"[..]));
    }

    #[test]
    fn ignores_unknown_attrs() {
        let f = parse_fetch(b"* 1 FETCH (UID 1 X-CUSTOM \"ignored\" MODSEQ (12345))\r\n");
        assert_eq!(f.uid, Some(1));
        assert!(f.flags.is_empty());
        assert!(f.size.is_none());
    }

    #[test]
    fn empty_flags_list() {
        let f = parse_fetch(b"* 1 FETCH (UID 7 FLAGS ())\r\n");
        assert_eq!(f.uid, Some(7));
        assert!(f.flags.is_empty());
    }
}
