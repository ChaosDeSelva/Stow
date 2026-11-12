//! Property-style edge tests over the public API: URL normalization idempotence, the
//! nesting bound, FTS query escaping, timestamp parsing, and the SSRF address table.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use stow_core::capture::normalize_url;
use stow_core::fetch::net::is_public_ip;
use stow_core::fetch::snapshot::{check_nesting, MAX_NESTING};
use stow_core::ids::parse_rfc3339;
use stow_core::repo::search;
use stow_core::{Db, StowError};

fn test_db() -> (tempfile::TempDir, Db) {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(dir.path()).unwrap();
    (dir, db)
}

// ---------------------------------------------------------------------------------------
// URL normalization
// ---------------------------------------------------------------------------------------

/// Tricky but valid http(s) URLs: IDN hosts, ports, userinfo, percent-encoding, trailing
/// dots, uppercase schemes, query ordering and oddities, tracking params, `YouTube` forms.
const TRICKY_URLS: &[&str] = &[
    "HTTPS://EXAMPLE.COM/Path",
    "https://example.com:443/",
    "http://example.com:80/",
    "https://example.com:8443/x",
    "https://user:pass@example.com/",
    "https://user:p%40ss@example.com/",
    "https://user@example.com:8080/x?y=1",
    "https://bücher.example/straße",
    "https://xn--bcher-kva.example/",
    "https://例え.テスト/パス?クエリ=値",
    "https://EXAMPLE.COM./",
    "https://example.com./a/./b/../c",
    "https://example.com/a%2Fb/c%20d",
    "https://example.com/a b",
    "https://example.com/%7Euser/",
    "https://example.com/~user/",
    "https://example.com/?b=2&a=1",
    "https://example.com/?a=1&a=2&a=1",
    "https://example.com/?q=a+b",
    "https://example.com/?q=a%20b",
    "https://example.com/?q=a%2Bb",
    "https://example.com/?flag",
    "https://example.com/?flag=",
    "https://example.com/?=value",
    "https://example.com/?&&a=1&&",
    "https://example.com/?a=1&utm_source=x&b=2&utm_medium=y",
    "https://example.com/?UTM_SOURCE=x",
    "https://example.com/?ref=foo&refx=1",
    "https://example.com/#frag",
    "https://example.com/?a=1#frag?b=2",
    "https://example.com/?q=%E2%9C%93",
    "https://example.com/?q=✓",
    "https://example.com/?q=a&b",
    "https://example.com/path?q=1&utm_campaign=",
    "https://[2001:db8::1]/x",
    "https://[2001:DB8::1]:8080/x",
    "https://192.168.0.1/x",
    "https://0x7f.1/",
    "  https://example.com/trim  ",
    "https://example.com",
    "https://example.com?x=1",
    "https://youtu.be/dQw4w9WgXcQ?si=abc&t=42s",
    "https://www.youtube.com/watch?v=abc&t=10&list=PL&index=2",
    "https://YOUTUBE.com/embed/abc123?autoplay=1",
    "https://m.youtube.com/shorts/xyz_-9",
    "https://x.com/user/status/123?s=20&t=abc&ref_src=tw",
    "https://twitter.com/user/status/123?S=20",
    "https://example.com/?q=%22quoted%22&r=%3Cb%3E",
    "https://example.com/;params?x=1",
    "https://example.com/a?q=1&q=1&utm_source=a&utm_source=b",
    "https://example.com/%E2%9C%93/%2e%2e/x",
    "https://example.com/?q=100%25",
    "https://example.com/?q=100%",
    "https://example.com/?a=b=c",
    "https://example.com/?a=1;b=2",
    "https://example.com/path/?",
    "https://example.com/p?q=%ZZ%4&r=%",
    "https://example.com/a%2e%2E/b",
    "https://example.com/?q=a%26b=c",
];

#[test]
fn normalize_url_is_idempotent_over_tricky_corpus() {
    assert!(TRICKY_URLS.len() >= 50);
    for raw in TRICKY_URLS {
        let once = normalize_url(raw).unwrap_or_else(|e| panic!("{raw}: {e}"));
        let twice = normalize_url(once.as_str())
            .unwrap_or_else(|e| panic!("re-normalizing {once} (from {raw}): {e}"));
        assert_eq!(once, twice, "not idempotent for {raw:?}");
        // The result is always a parseable http(s) URL with a host and no fragment.
        assert!(matches!(once.scheme(), "http" | "https"), "{raw}");
        assert!(once.host_str().is_some(), "{raw}");
        assert!(once.fragment().is_none(), "{raw}");
        // Nothing the user typed leaks a tracking key through.
        assert!(
            !once
                .query_pairs()
                .any(|(k, _)| k.starts_with("utm_") || k == "fbclid" || k == "ref"),
            "tracking key survived in {once}"
        );
    }
}

#[test]
fn normalize_url_specific_canonical_forms() {
    let n = |u: &str| normalize_url(u).unwrap().to_string();
    assert_eq!(n("HTTPS://EXAMPLE.COM/Path"), "https://example.com/Path");
    assert_eq!(n("https://example.com:443/"), "https://example.com/");
    assert_eq!(n("http://example.com:80/"), "http://example.com/");
    assert_eq!(
        n("https://example.com:8443/x"),
        "https://example.com:8443/x"
    );
    assert_eq!(
        n("https://bücher.example/straße"),
        "https://xn--bcher-kva.example/stra%C3%9Fe"
    );
    assert_eq!(n("https://EXAMPLE.COM./"), "https://example.com./");
    assert_eq!(
        n("https://example.com./a/./b/../c"),
        "https://example.com./a/c"
    );
    assert_eq!(
        n("https://[2001:DB8::1]:8080/x"),
        "https://[2001:db8::1]:8080/x"
    );
    assert_eq!(n("https://0x7f.1/"), "https://127.0.0.1/");
    assert_eq!(
        n("https://example.com/?b=2&a=1"),
        "https://example.com/?b=2&a=1"
    );
    assert_eq!(
        n("https://example.com/?a=1&utm_source=x&b=2&utm_medium=y"),
        "https://example.com/?a=1&b=2"
    );
    assert_eq!(
        n("https://example.com/?ref=foo&refx=1"),
        "https://example.com/?refx=1"
    );
    assert_eq!(n("https://example.com/path/?"), "https://example.com/path/");
    assert_eq!(
        n("https://example.com/?a=1#frag?b=2"),
        "https://example.com/?a=1"
    );
    assert_eq!(
        n("https://user:p%40ss@example.com/"),
        "https://user:p%40ss@example.com/"
    );
    assert_eq!(
        n("https://youtu.be/dQw4w9WgXcQ?si=abc&t=42s"),
        "https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=42s"
    );
    assert_eq!(
        n("https://YOUTUBE.com/embed/abc123?autoplay=1"),
        "https://www.youtube.com/watch?v=abc123"
    );
    assert_eq!(
        n("https://twitter.com/user/status/123?S=20"),
        "https://twitter.com/user/status/123?S=20",
        "tracking keys are case-sensitive; an uppercase one is not ours to drop"
    );
}

#[test]
fn normalize_url_rejects_hostless_and_non_http_forms() {
    // Note: `https:///path` is NOT rejected; the WHATWG parser (like browsers) collapses
    // the extra slash and reads `path` as the host.
    for bad in [
        "https://",
        "http://",
        "mailto:a@b.c",
        "data:text/html,hi",
        "//example.com/x",
        "example.com/x",
        "ftp://example.com/x",
        "https://exa mple.com/",
        "https://[::1",
        "https://example.com:99999/",
    ] {
        let err = normalize_url(bad).expect_err(bad);
        assert!(
            matches!(err, StowError::InvalidUrl { .. }),
            "{bad}: {err:?}"
        );
        assert_eq!(err.code(), "invalid_url", "{bad}");
    }
}

// ---------------------------------------------------------------------------------------
// check_nesting
// ---------------------------------------------------------------------------------------

fn nested(tag: &str, depth: usize, filler: &str) -> String {
    let mut s = String::with_capacity(depth * (tag.len() * 2 + 6 + filler.len()));
    for _ in 0..depth {
        s.push('<');
        s.push_str(tag);
        s.push('>');
        s.push_str(filler);
    }
    s.push_str("leaf");
    for _ in 0..depth {
        s.push_str("</");
        s.push_str(tag);
        s.push('>');
    }
    s
}

#[test]
fn check_nesting_boundary_at_max_nesting() {
    assert_eq!(MAX_NESTING, 512);
    for (depth, ok) in [(1, true), (511, true), (512, true), (513, false)] {
        for tag in ["div", "DIV", "section", "my-element", "a:b"] {
            let html = format!("<html><body>{}</body></html>", nested(tag, depth, ""));
            let r = check_nesting(&html);
            assert_eq!(r.is_ok(), ok, "<{tag}> × {depth}: {r:?}");
            if !ok {
                assert!(
                    matches!(&r, Err(StowError::Extract(m)) if m == "nesting too deep"),
                    "{r:?}"
                );
            }
        }
    }
    // Unclosed tags count the same as closed ones.
    assert!(check_nesting(&"<div>".repeat(512)).is_ok());
    assert!(check_nesting(&"<div>".repeat(513)).is_err());
    assert!(check_nesting("").is_ok());
    assert!(check_nesting("plain text without tags < 3 > 2").is_ok());
}

#[test]
fn check_nesting_ignores_void_and_flat_tags_at_every_level() {
    // One void/flat tag of every kind between each level: still exactly `depth` deep.
    let filler = "<br><img src=x><input><hr/><meta charset=utf-8><link rel=x><wbr><p><li><td><tr>";
    assert!(check_nesting(&nested("div", 512, filler)).is_ok());
    assert!(check_nesting(&nested("div", 513, filler)).is_err());
    // Void tags alone never add depth, however many.
    assert!(check_nesting(&"<br>".repeat(10_000)).is_ok());
    assert!(check_nesting(&"<img src=a.png>".repeat(10_000)).is_ok());
    assert!(check_nesting(&"<p>".repeat(10_000)).is_ok());
    // Self-closing syntax outside <svg>/<math> is still an opening tag (HTML ignores "/>").
    assert!(check_nesting(&"<div/>".repeat(513)).is_err());
    assert!(check_nesting(&"<div/>".repeat(512)).is_ok());
    // Inside <svg>, "/>" really self-closes.
    let svg = format!("<svg>{}</svg>", "<path d=M0/>".repeat(10_000));
    assert!(check_nesting(&svg).is_ok());
    // Raw-text elements and comments full of tags do not count.
    let raw = format!(
        "<div><script>{}</script><style>{}</style><!-- {} --></div>",
        "<div>".repeat(1000),
        "<section>".repeat(1000),
        "<article>".repeat(1000)
    );
    assert!(check_nesting(&raw).is_ok());
    // ... but a closed raw-text element followed by real nesting still counts.
    let after_script = format!("<script>x</script>{}", "<div>".repeat(513));
    assert!(check_nesting(&after_script).is_err());
}

// ---------------------------------------------------------------------------------------
// FTS query escaping
// ---------------------------------------------------------------------------------------

/// Two snapshot docs: one full of FTS5 syntax, one full of non-ASCII text.
fn search_db_with_operator_and_unicode_docs() -> (tempfile::TempDir, Db) {
    let (d, db) = test_db();
    search::index(
        &db,
        "snapshot",
        "ops",
        Some("ops"),
        None,
        "Operators",
        "config foo:bar value cats AND dogs OR birds NOT fish NEAR(x) a+b -minus ^caret \
         {braces} (parens) \"quoted\" star* colon: comma, semicolon; pipe|",
    )
    .unwrap();
    search::index(
        &db,
        "snapshot",
        "uni",
        Some("uni"),
        None,
        "Unicode café",
        "naïve résumé Straße 東京タワー Привет мир مرحبا שלום 🦓 zebra emoji ﬁligree",
    )
    .unwrap();
    (d, db)
}

#[test]
fn search_neutralizes_every_fts5_operator() {
    let (_d, db) = search_db_with_operator_and_unicode_docs();
    // Every FTS5 operator / syntax element, alone and glued to words: never an error.
    let operators = [
        "AND",
        "OR",
        "NOT",
        "NEAR",
        "+",
        "-",
        "^",
        ":",
        "*",
        "\"",
        "(",
        ")",
        "{",
        "}",
        ",",
        "title:",
        "body:",
        "{title body}:",
        "-title:",
        "NEAR(",
        "NEAR(a b, 5)",
        "\"\"",
        "\"\"\"",
        "*\"",
        "\"*",
        "a:b:c",
        "::",
        "^^",
        "--",
        "++",
        "()",
        "{}",
        "(a OR b)",
        "a AND (b OR c)",
        "title : cats",
        "{title}:cats",
        "-\"cats\"",
        "cats*dogs",
        "\"cats dogs\"",
        "cats ^dogs",
        "cats + dogs",
        "NEAR(cats dogs, 2)",
        "foo:bar",
    ];
    for q in operators {
        let r = search::query(&db, q, 10);
        assert!(r.is_ok(), "{q:?}: {:?}", r.err());
    }
    // Operators are plain words that must all appear (AND semantics, prefix match).
    assert_eq!(search::query(&db, "foo:bar", 10).unwrap().len(), 1);
    assert_eq!(search::query(&db, "NEAR(x)", 10).unwrap().len(), 1);
    assert_eq!(search::query(&db, "{braces}", 10).unwrap().len(), 1);
    assert_eq!(search::query(&db, "-minus", 10).unwrap().len(), 1);
    assert_eq!(search::query(&db, "^caret", 10).unwrap().len(), 1);
    assert_eq!(search::query(&db, "cats NOT dogs", 10).unwrap().len(), 1);
    assert!(search::query(&db, "cats NOT platypus", 10)
        .unwrap()
        .is_empty());
    assert!(
        search::query(&db, "title:Unicode", 10).unwrap().is_empty(),
        "a column filter is not interpreted; 'title' must appear in the text"
    );
}

#[test]
fn search_handles_unicode_text_and_symbol_only_queries() {
    let (_d, db) = search_db_with_operator_and_unicode_docs();
    // Unicode: diacritics fold, scripts tokenize, prefix works, symbols never error.
    for (q, want) in [
        ("café", 1),
        ("cafe", 1),
        ("CAFÉ", 1),
        ("naive", 1),
        ("resume", 1),
        ("Straße", 1),
        ("東京", 1),
        ("東京タワー", 1),
        ("Привет", 1),
        ("привет", 1),
        ("мир", 1),
        ("مرحبا", 1),
        ("שלום", 1),
        ("zebra", 1),
        // unicode61 folds diacritics but not compatibility ligatures: the exact `ﬁ`
        // form matches; the decomposed "filigree" is not expected to.
        ("ﬁligree", 1),
        ("quokka", 0),
    ] {
        let hits = search::query(&db, q, 10).unwrap();
        assert_eq!(hits.len(), want, "{q:?}: {hits:?}");
        if want == 1 {
            assert_eq!(hits[0].doc_id, "uni", "{q:?}");
        }
    }
    for symbols in ["🦓", "🦓🦓", "…", "—", "«»", "\u{200b}", "\u{feff}", "¿?¡!"] {
        let r = search::query(&db, symbols, 10);
        assert!(r.is_ok(), "{symbols:?}: {:?}", r.err());
    }
    // Whitespace of every kind splits words; nothing else does.
    assert_eq!(
        search::query(&db, "cats\tdogs\nbirds\u{a0}fish\u{2003}config", 10)
            .unwrap()
            .len(),
        1
    );
    assert!(search::query(&db, "   ", 10).unwrap().is_empty());
}

#[test]
fn search_limit_is_honoured_and_order_is_stable() {
    let (_d, db) = test_db();
    for i in 0..60 {
        let title = format!("Doc {i}");
        let body = format!("common word {} {}", "tail ".repeat(i % 7), i);
        search::index(
            &db,
            "snapshot",
            &format!("d{i}"),
            Some(&format!("d{i}")),
            None,
            &title,
            &body,
        )
        .unwrap();
    }
    for limit in [0, 1, 10, 59, 60, 61, 1000] {
        let hits = search::query(&db, "common", limit).unwrap();
        assert_eq!(hits.len(), limit.min(60), "limit {limit}");
    }
    let first = search::query(&db, "common", 60).unwrap();
    for _ in 0..5 {
        let again = search::query(&db, "common", 60).unwrap();
        assert_eq!(again, first, "ordering changed between identical queries");
    }
    let ids: std::collections::HashSet<_> = first.iter().map(|h| h.doc_id.as_str()).collect();
    assert_eq!(ids.len(), 60, "no duplicates");
    // A smaller limit is a prefix of the larger result.
    let ten = search::query(&db, "common", 10).unwrap();
    assert_eq!(ten.as_slice(), &first[..10]);
}

// ---------------------------------------------------------------------------------------
// parse_rfc3339
// ---------------------------------------------------------------------------------------

fn is_canonical(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 27
        && b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b'T'
        && b[13] == b':'
        && b[16] == b':'
        && b[19] == b'.'
        && b[26] == b'Z'
        && b.iter()
            .enumerate()
            .all(|(i, c)| matches!(i, 4 | 7 | 10 | 13 | 16 | 19 | 26) || c.is_ascii_digit())
}

#[test]
fn parse_rfc3339_round_trips_offsets_and_fractions() {
    let cases = [
        ("2026-06-15T12:34:56Z", "2026-06-15T12:34:56.000000Z"),
        ("2026-06-15T12:34:56+00:00", "2026-06-15T12:34:56.000000Z"),
        ("2026-06-15T12:34:56-00:00", "2026-06-15T12:34:56.000000Z"),
        ("2026-06-15T12:34:56+05:30", "2026-06-15T07:04:56.000000Z"),
        ("2026-06-15T12:34:56-09:30", "2026-06-15T22:04:56.000000Z"),
        ("2026-06-15T00:30:00+14:00", "2026-06-14T10:30:00.000000Z"),
        ("2026-06-15T23:30:00-12:00", "2026-06-16T11:30:00.000000Z"),
        (
            "2026-12-31T23:59:59.999999-01:00",
            "2027-01-01T00:59:59.999999Z",
        ),
        ("2026-06-15T12:34:56.1Z", "2026-06-15T12:34:56.100000Z"),
        ("2026-06-15T12:34:56.12Z", "2026-06-15T12:34:56.120000Z"),
        ("2026-06-15T12:34:56.123Z", "2026-06-15T12:34:56.123000Z"),
        ("2026-06-15T12:34:56.123456Z", "2026-06-15T12:34:56.123456Z"),
        (
            "2026-06-15T12:34:56.1234567Z",
            "2026-06-15T12:34:56.123456Z",
        ),
        (
            "2026-06-15T12:34:56.123456789Z",
            "2026-06-15T12:34:56.123456Z",
        ),
        (
            "2026-06-15T12:34:56.9999999Z",
            "2026-06-15T12:34:56.999999Z",
        ),
        (
            "2026-06-15T12:34:56.000000001Z",
            "2026-06-15T12:34:56.000000Z",
        ),
        ("2024-02-29T00:00:00Z", "2024-02-29T00:00:00.000000Z"),
        ("1970-01-01T00:00:00Z", "1970-01-01T00:00:00.000000Z"),
        ("1969-12-31T23:59:59Z", "1969-12-31T23:59:59.000000Z"),
        ("9999-12-31T23:59:59Z", "9999-12-31T23:59:59.000000Z"),
        ("  2026-06-15T12:34:56Z\n", "2026-06-15T12:34:56.000000Z"),
    ];
    for (input, want) in cases {
        let got = parse_rfc3339(input).unwrap_or_else(|e| panic!("{input:?}: {e}"));
        assert_eq!(got, want, "{input:?}");
        assert!(is_canonical(&got), "{got}");
        assert_eq!(
            parse_rfc3339(&got).unwrap(),
            got,
            "canonical form must be a fixed point"
        );
    }
    // RFC 3339 §5.6 permits lowercase `t`/`z` and a space instead of `T`; whichever of
    // those chrono accepts, the output must still be canonical.
    for lax in [
        "2026-06-15t12:34:56z",
        "2026-06-15T12:34:56z",
        "2026-06-15 12:34:56Z",
    ] {
        if let Ok(c) = parse_rfc3339(lax) {
            assert_eq!(c, "2026-06-15T12:34:56.000000Z", "{lax:?}");
        }
    }
    assert_eq!(
        parse_rfc3339("2026-06-15 12:34:56Z").unwrap(),
        "2026-06-15T12:34:56.000000Z",
        "space separator is accepted (and canonicalized to T)"
    );
}

#[test]
fn parse_rfc3339_rejects_garbage() {
    for bad in [
        "",
        " ",
        "now",
        "tomorrow",
        "1700000000",
        "2026-01-01",
        "2026-01-01T00:00:00",
        "2026-01-01T00:00Z",
        "2026-13-01T00:00:00Z",
        "2026-00-10T00:00:00Z",
        "2026-02-30T00:00:00Z",
        "2023-02-29T00:00:00Z",
        "2026-01-01T24:00:00Z",
        "2026-01-01T00:60:00Z",
        "2026-01-01T00:00:00+24:00",
        "2026-01-01T00:00:00+05:60",
        "2026-01-01T00:00:00Z extra",
        "x2026-01-01T00:00:00Z",
        "2026-01-01T00:00:00.Z",
        "2026-01-01T00:00:00,5Z",
        "2026-1-1T00:00:00Z",
        "26-01-01T00:00:00Z",
        "2026-01-01T0:00:00Z",
        "2026-01-01T00:00:00+5:00",
        "2026-01-01T00:00:00UTC",
        "2026-01-01T00:00:00 Z",
        "Mon, 01 Jan 2026 00:00:00 GMT",
        "2026-01-01T00:00:00Z\u{0}",
        "٢٠٢٦-01-01T00:00:00Z",
    ] {
        let err = parse_rfc3339(bad).expect_err(bad);
        assert!(
            matches!(err, StowError::InvalidInput { field: "timestamp", ref value } if value == bad),
            "{bad:?}: {err:?}"
        );
        assert_eq!(err.code(), "invalid_input");
    }
}

#[test]
fn parse_rfc3339_canonical_strings_sort_chronologically() {
    // Same instants written in different offsets, plus neighbours a microsecond apart.
    let inputs = [
        "2026-06-15T12:00:00+02:00",
        "2026-06-15T10:00:00.000001Z",
        "2026-06-15T05:59:59.999999-04:00",
        "2026-06-15T19:00:00+09:00",
        "2026-06-14T23:00:00-11:00",
        "2026-06-15T10:00:00Z",
        "2025-12-31T23:59:59.5-00:00",
        "2026-06-15T09:59:59.9999999Z",
    ];
    let mut canon: Vec<String> = inputs.iter().map(|s| parse_rfc3339(s).unwrap()).collect();
    let mut by_time: Vec<(chrono::DateTime<chrono::Utc>, String)> = inputs
        .iter()
        .map(|s| {
            let t = chrono::DateTime::parse_from_rfc3339(s)
                .unwrap()
                .with_timezone(&chrono::Utc);
            (t, parse_rfc3339(s).unwrap())
        })
        .collect();
    canon.sort();
    by_time.sort();
    let want: Vec<String> = by_time.into_iter().map(|(_, s)| s).collect();
    assert_eq!(canon, want, "lexical order must equal chronological order");
    // Two different offsets for the same instant collapse to one string.
    assert_eq!(
        parse_rfc3339("2026-06-15T12:00:00+02:00").unwrap(),
        parse_rfc3339("2026-06-15T19:00:00+09:00").unwrap()
    );
}

// ---------------------------------------------------------------------------------------
// is_public_ip
// ---------------------------------------------------------------------------------------

fn v4(s: &str) -> Ipv4Addr {
    s.parse().unwrap()
}

fn six_to_four(ip: Ipv4Addr) -> Ipv6Addr {
    let n = u32::from(ip);
    Ipv6Addr::new(0x2002, (n >> 16) as u16, (n & 0xffff) as u16, 0, 0, 0, 0, 1)
}

fn nat64(ip: Ipv4Addr) -> Ipv6Addr {
    let n = u32::from(ip);
    Ipv6Addr::new(
        0x64,
        0xff9b,
        0,
        0,
        0,
        0,
        (n >> 16) as u16,
        (n & 0xffff) as u16,
    )
}

#[test]
fn is_public_ip_agrees_across_ipv4_embeddings() {
    let privates = [
        "127.0.0.1",
        "127.255.255.254",
        "10.0.0.1",
        "10.255.255.255",
        "172.16.0.1",
        "172.31.255.255",
        "192.168.0.1",
        "192.168.255.255",
        "169.254.169.254",
        "169.254.0.1",
        "100.64.0.1",
        "100.127.255.255",
        "0.0.0.0",
        "0.1.2.3",
        "192.0.0.1",
        "192.0.2.1",
        "198.18.0.1",
        "198.19.255.255",
        "198.51.100.1",
        "203.0.113.1",
        "224.0.0.1",
        "239.255.255.255",
        "240.0.0.1",
        "255.255.255.255",
    ];
    let publics = [
        "8.8.8.8",
        "1.1.1.1",
        "9.255.255.255",
        "11.0.0.1",
        "172.15.255.255",
        "172.32.0.1",
        "192.167.255.255",
        "192.169.0.1",
        "169.253.255.255",
        "169.255.0.1",
        "100.63.255.255",
        "100.128.0.1",
        "192.0.1.1",
        "192.0.3.1",
        "198.17.255.255",
        "198.20.0.1",
        "223.255.255.255",
        "93.184.216.34",
    ];
    for s in privates {
        let ip = v4(s);
        assert!(!is_public_ip(IpAddr::V4(ip)), "{s} v4");
        assert!(!is_public_ip(IpAddr::V6(ip.to_ipv6_mapped())), "::ffff:{s}");
        assert!(!is_public_ip(IpAddr::V6(six_to_four(ip))), "6to4 of {s}");
        assert!(!is_public_ip(IpAddr::V6(nat64(ip))), "nat64 of {s}");
        assert!(
            !is_public_ip(IpAddr::V6(ip.to_ipv6_compatible())),
            "::{s} (compatible)"
        );
    }
    for s in publics {
        let ip = v4(s);
        assert!(is_public_ip(IpAddr::V4(ip)), "{s} v4");
        assert!(is_public_ip(IpAddr::V6(ip.to_ipv6_mapped())), "::ffff:{s}");
        assert!(is_public_ip(IpAddr::V6(six_to_four(ip))), "6to4 of {s}");
        assert!(is_public_ip(IpAddr::V6(nat64(ip))), "nat64 of {s}");
    }
}

#[test]
fn is_public_ip_v6_special_ranges() {
    let cases: &[(&str, bool)] = &[
        // Literal spellings of the embeddings above.
        ("::ffff:192.168.1.1", false),
        ("::ffff:172.16.0.1", false),
        ("::ffff:100.64.0.1", false),
        ("::ffff:0.0.0.0", false),
        ("::ffff:c0a8:101", false),
        ("::ffff:1.1.1.1", true),
        ("2002:c0a8:101::", false),
        ("2002:a00:1::", false),
        ("2002:7f00:1::", false),
        ("2002:a9fe:a9fe::", false),
        ("2002:ac10:1::1", false),
        ("2002:6440:1::", false),
        ("2002:808:808::", true),
        ("2002:101:101::1", true),
        ("64:ff9b::7f00:1", false),
        ("64:ff9b::a00:1", false),
        ("64:ff9b::c0a8:1", false),
        ("64:ff9b::808:808", true),
        ("64:ff9b::1.1.1.1", true),
        ("::7f00:1", false),
        ("::c0a8:1", false),
        ("::808:808", false),
        // Native v6 ranges.
        ("::1", false),
        ("::", false),
        ("fc00::1", false),
        ("fdff:ffff::1", false),
        ("fe80::1", false),
        ("febf::1", false),
        ("fec0::1", false),
        ("ff02::1", false),
        ("ff0e::1", false),
        ("2001:db8::1", false),
        ("2001::1", false),
        ("2001:0:4136:e378:8000:63bf:3fff:fdd2", false),
        ("2001:4860:4860::8888", true),
        ("2606:4700:4700::1111", true),
        ("2a00:1450:4001:80b::200e", true),
        ("2001:db9::1", true),
        ("2001:1::1", true),
        ("fbff::1", true),
        ("fe7f::1", true),
        ("ff00::", false),
    ];
    for (s, public) in cases {
        let ip: Ipv6Addr = s.parse().unwrap_or_else(|e| panic!("{s}: {e}"));
        assert_eq!(is_public_ip(IpAddr::V6(ip)), *public, "{s}");
    }
}
