//! Emphasis markers follow CommonMark's flanking rule, so an identifier or a
//! file name in a chat message is never read as italic.

use chat::{Mark, message::inline_spans};

/// `(text, [(span text, italic?, bold?)])`
type Row = (&'static str, &'static [(&'static str, bool, bool)]);

const ROWS: &[Row] = &[
    ("my_var_name", &[("my_var_name", false, false)]),
    (
        "see src/my_file_name.rs now",
        &[("see src/my_file_name.rs now", false, false)],
    ),
    ("__init__", &[("init", false, true)]),
    ("a__b__c", &[("a__b__c", false, false)]),
    ("_var_", &[("var", true, false)]),
    (
        "say _this_ now",
        &[
            ("say ", false, false),
            ("this", true, false),
            (" now", false, false),
        ],
    ),
    ("__bold__", &[("bold", false, true)]),
    (
        "un*believ*able",
        &[
            ("un", false, false),
            ("believ", true, false),
            ("able", false, false),
        ],
    ),
    ("a * b * c", &[("a * b * c", false, false)]),
    ("2 ** 3 ** 4", &[("2 ** 3 ** 4", false, false)]),
    (
        "*it* and **bo**",
        &[
            ("it", true, false),
            (" and ", false, false),
            ("bo", false, true),
        ],
    ),
];

#[test]
fn emphasis_follows_the_flanking_rule() {
    for (text, expected) in ROWS {
        let got: Vec<(String, bool, bool)> = inline_spans(text)
            .into_iter()
            .map(|span| {
                let italic = span.marks.contains(&Mark::Italic);
                let bold = span.marks.contains(&Mark::Bold);
                (span.text, italic, bold)
            })
            .collect();
        let want: Vec<(String, bool, bool)> = expected
            .iter()
            .map(|(text, italic, bold)| (text.to_string(), *italic, *bold))
            .collect();
        assert_eq!(got, want, "for `{text}`");
    }
}
