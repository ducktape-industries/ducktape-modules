//! The self-test harness: every unit test in this crate as a plain
//! `fn()` in one table, so the same cases run natively under libtest
//! (`cargo test`) AND inside a wasm32-unknown-unknown module under a bare
//! runtime that has no libtest — three C-ABI exports, no host imports
//! (`wasm-selftest.js` beside the crate drives them under node).
//!
//! A case that fails asserts, which under the guest's abort-on-panic
//! strategy is an `unreachable` trap the runtime reports as an error.

pub const CASES: &[(&str, fn())] = &[
    (
        "voice::codec::round_trip_preserves_energy",
        crate::voice::codec::tests::round_trip_preserves_energy,
    ),
    (
        "voice::codec::a_sixty_millisecond_toc_is_refused_and_the_lane_keeps_decoding",
        crate::voice::codec::tests::a_sixty_millisecond_toc_is_refused_and_the_lane_keeps_decoding,
    ),
    (
        "voice::codec::no_short_packet_panics",
        crate::voice::codec::tests::no_short_packet_panics,
    ),
    (
        "voice::media::golden_header_be",
        crate::voice::media::tests::golden_header_be,
    ),
    (
        "voice::media::frame_round_trip",
        crate::voice::media::tests::frame_round_trip,
    ),
    (
        "voice::media::rejects_garbage",
        crate::voice::media::tests::rejects_garbage,
    ),
    (
        "voice::jitter::plays_in_order_after_prefill",
        crate::voice::jitter::tests::plays_in_order_after_prefill,
    ),
    (
        "voice::jitter::reordered_arrival_is_absorbed",
        crate::voice::jitter::tests::reordered_arrival_is_absorbed,
    ),
    (
        "voice::jitter::isolated_loss_reports_gap_and_stays_aligned",
        crate::voice::jitter::tests::isolated_loss_reports_gap_and_stays_aligned,
    ),
    (
        "voice::jitter::multi_frame_gap_reports_each_missing_tick",
        crate::voice::jitter::tests::multi_frame_gap_reports_each_missing_tick,
    ),
    (
        "voice::jitter::underrun_grows_depth_and_refills",
        crate::voice::jitter::tests::underrun_grows_depth_and_refills,
    ),
    (
        "voice::jitter::late_packet_is_dropped_not_replayed",
        crate::voice::jitter::tests::late_packet_is_dropped_not_replayed,
    ),
    (
        "voice::jitter::seq_wraparound_is_seamless",
        crate::voice::jitter::tests::seq_wraparound_is_seamless,
    ),
    (
        "voice::engine::second_speaker_raises_mix_energy",
        crate::voice::engine::tests::second_speaker_raises_mix_energy,
    ),
    (
        "voice::engine::a_new_epoch_reopens_the_lane_without_a_roster_departure",
        crate::voice::engine::tests::a_new_epoch_reopens_the_lane_without_a_roster_departure,
    ),
    (
        "voice::engine::same_epoch_seq_restart_is_counted_late",
        crate::voice::engine::tests::same_epoch_seq_restart_is_counted_late,
    ),
    (
        "voice::engine::forgetting_a_departed_peer_drops_the_lane",
        crate::voice::engine::tests::forgetting_a_departed_peer_drops_the_lane,
    ),
    (
        "voice::engine::hostile_and_truncated_datagrams_are_counted_not_fatal",
        crate::voice::engine::tests::hostile_and_truncated_datagrams_are_counted_not_fatal,
    ),
    (
        "voice::engine::sent_frames_carry_epoch_seq_and_timestamp",
        crate::voice::engine::tests::sent_frames_carry_epoch_seq_and_timestamp,
    ),
    (
        "video::frame::golden_header_be",
        crate::video::frame::tests::golden_header_be,
    ),
    (
        "video::frame::fragments_round_trip",
        crate::video::frame::tests::fragments_round_trip,
    ),
    (
        "video::frame::exact_multiple_has_no_empty_tail",
        crate::video::frame::tests::exact_multiple_has_no_empty_tail,
    ),
    (
        "video::frame::empty_and_oversize_inputs_error",
        crate::video::frame::tests::empty_and_oversize_inputs_error,
    ),
    (
        "video::frame::decode_rejects_truncated",
        crate::video::frame::tests::decode_rejects_truncated,
    ),
    (
        "video::frame::decode_rejects_zero_frag_count",
        crate::video::frame::tests::decode_rejects_zero_frag_count,
    ),
    (
        "video::frame::decode_rejects_index_out_of_range",
        crate::video::frame::tests::decode_rejects_index_out_of_range,
    ),
    (
        "video::frame::frame_no_comparison_wraps",
        crate::video::frame::tests::frame_no_comparison_wraps,
    ),
    (
        "video::assembly::out_of_order_fragments_complete",
        crate::video::assembly::tests::out_of_order_fragments_complete,
    ),
    (
        "video::assembly::single_fragment_frame_completes_immediately",
        crate::video::assembly::tests::single_fragment_frame_completes_immediately,
    ),
    (
        "video::assembly::incomplete_frame_replaced_by_newer_counts_as_dropped",
        crate::video::assembly::tests::incomplete_frame_replaced_by_newer_counts_as_dropped,
    ),
    (
        "video::assembly::stale_older_or_duplicate_fragments_are_ignored",
        crate::video::assembly::tests::stale_older_or_duplicate_fragments_are_ignored,
    ),
    (
        "video::assembly::completed_frame_nos_are_monotonic",
        crate::video::assembly::tests::completed_frame_nos_are_monotonic,
    ),
    (
        "call_wire::golden_captured_video_be",
        crate::call_wire::tests::golden_captured_video_be,
    ),
    (
        "call_wire::golden_peer_video_be",
        crate::call_wire::tests::golden_peer_video_be,
    ),
    (
        "call_wire::an_audio_frame_carries_its_payload_opaquely",
        crate::call_wire::tests::an_audio_frame_carries_its_payload_opaquely,
    ),
    (
        "call_wire::short_and_wrong_tag_frames_decode_to_none",
        crate::call_wire::tests::short_and_wrong_tag_frames_decode_to_none,
    ),
    (
        "control::beacon_round_trips_and_rejects_other_frames",
        crate::control::tests::beacon_round_trips_and_rejects_other_frames,
    ),
    (
        "realtime::init_refuses_a_bad_self_peer_and_a_binding_without_voice",
        crate::realtime::tests::init_refuses_a_bad_self_peer_and_a_binding_without_voice,
    ),
    (
        "realtime::flows_derive_like_the_data_plane",
        crate::realtime::tests::flows_derive_like_the_data_plane,
    ),
    (
        "realtime::a_channel_opens_its_flows_with_its_first_seat_and_closes_them_with_its_last",
        crate::realtime::tests::a_channel_opens_its_flows_with_its_first_seat_and_closes_them_with_its_last,
    ),
    (
        "realtime::the_roster_minus_self_is_the_admission_set_on_both_lanes",
        crate::realtime::tests::the_roster_minus_self_is_the_admission_set_on_both_lanes,
    ),
    (
        "realtime::audio_is_stamped_out_to_the_roster_and_delivered_to_who_admits_the_sender",
        crate::realtime::tests::audio_is_stamped_out_to_the_roster_and_delivered_to_who_admits_the_sender,
    ),
    (
        "realtime::pictures_fragment_out_and_reassemble_in_per_sender",
        crate::realtime::tests::pictures_fragment_out_and_reassemble_in_per_sender,
    ),
    (
        "realtime::beacons_go_out_now_and_every_second_and_come_in_as_peer_beacons",
        crate::realtime::tests::beacons_go_out_now_and_every_second_and_come_in_as_peer_beacons,
    ),
    (
        "realtime::a_malformed_client_frame_closes_that_session_only",
        crate::realtime::tests::a_malformed_client_frame_closes_that_session_only,
    ),
];

#[cfg(feature = "selftest")]
mod exports {
    use super::CASES;

    #[unsafe(no_mangle)]
    pub extern "C" fn selftest_count() -> u32 {
        CASES.len() as u32
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn selftest_name_ptr(index: u32) -> *const u8 {
        CASES[index as usize].0.as_ptr()
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn selftest_name_len(index: u32) -> u32 {
        CASES[index as usize].0.len() as u32
    }

    /// Runs one case; returns on success, traps on failure.
    #[unsafe(no_mangle)]
    pub extern "C" fn selftest_run(index: u32) {
        (CASES[index as usize].1)()
    }
}

#[cfg(test)]
mod tests {
    /// the table names every `#[cfg_attr(test, test)]` case, so the wasm run
    /// is the native run — not a subset of it.
    #[test]
    fn table_is_complete() {
        let src = [
            include_str!("voice/codec.rs"),
            include_str!("voice/media.rs"),
            include_str!("voice/jitter.rs"),
            include_str!("voice/engine.rs"),
            include_str!("video/frame.rs"),
            include_str!("video/assembly.rs"),
            include_str!("call_wire.rs"),
            include_str!("control.rs"),
            include_str!("realtime.rs"),
        ];
        let declared: usize = src
            .iter()
            .map(|s| s.matches("#[cfg_attr(test, test)]").count())
            .sum();
        assert_eq!(declared, super::CASES.len());
    }
}
