use super::*;

#[test]
fn an_op_reads_as_its_program_describes_it_through_the_host() {
    let (mut cx, _) = ready();
    // Ada's post, as chat described it
    assert!(cx.has_text("Post in #design"), "{:?}", cx.texts());
    // one the host could not describe reads as its bytes
    assert!(cx.has_text("mystery · 4 bytes"), "{:?}", cx.texts());
    let asked = cx.host().requests::<ModuleDescribe>();
    assert!(asked.contains(&("mystery".to_owned(), vec![1, 2, 3, 4])));

    // a dm post: its title, the two accounts by name and link
    cx.simulate_click(&format!("explorer-tx-{}", abi::hex(&[0xc3; 32])));
    cx.run_until_parked();
    let texts = cx.texts();
    assert!(cx.has_text("Direct message"), "{texts:?}");
    assert!(!texts.iter().any(|t| t.contains("↔")), "{texts:?}");
    assert!(cx.has_text("between") && cx.has_text("Ada") && cx.has_text("account 7"));
    cx.simulate_click("explorer-value-1-0");
    cx.run_until_parked();
    assert!(
        cx.has_text("laptop"),
        "the account link opens Ada: {:?}",
        cx.texts()
    );
    cx.assert_accessible();
}

#[test]
fn an_undescribed_op_shows_its_size_and_bytes() {
    let (mut cx, _) = ready();
    cx.simulate_click(&format!("explorer-tx-{}", abi::hex(&[0xb2; 32])));
    cx.run_until_parked();
    assert!(cx.has_text("mystery · 4 bytes"), "{:?}", cx.texts());
    assert!(cx.has_text("bytes") && cx.has_text("01020304"));
    assert_eq!(decode::bytes("chat", &[0xff; 3]).title, "chat · 3 bytes");
}

#[test]
fn values_read_as_a_person_reads_them() {
    assert_eq!(decode::preview(0, &[]), "0 bytes");
    let push = |len: usize| match Value::bytes(&vec![7; len]) {
        Value::Bytes { len, preview } => decode::preview(len, &preview),
        _ => unreachable!(),
    };
    assert_eq!(push(100), "100 bytes · 07070707…0707");
    assert_eq!(push(1 << 20), "1048576 bytes · 07070707…0707");
    assert_eq!(push(4), "07070707");
    assert_eq!(decode::amount(123_456_789, 2), "1,234,567.89");
    assert_eq!(decode::amount(5, 3), "0.005");
    assert_eq!(decode::amount(42, 0), "42");
}

#[test]
fn numbers_hashes_and_times_read_as_a_person_reads_them() {
    assert_eq!(decode::grouped(6230), "6,230");
    assert_eq!(decode::grouped(1_000_000), "1,000,000");
    assert_eq!(decode::grouped(12), "12");
    assert_eq!(decode::short(&[0xab; 32]), "abababab…abab");
    assert_eq!(decode::ago(10_000, 8_000), "2s");
    assert_eq!(decode::ago(4_000_000, 0), "1h");
    assert_eq!(decode::date(0), "1 Jan 1970, 00:00:00");
    assert_eq!(decode::date(1_790_236_327_000), "24 Sep 2026, 07:52:07");
    assert_eq!(decode::date(951_782_400_000), "29 Feb 2000, 00:00:00");
}
