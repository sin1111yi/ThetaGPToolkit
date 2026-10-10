//! The wire's contract, byte by byte: the layout a frame is, and what the
//! receive side does with the bytes it is fed.

use thetagp_wire as wire;
use wire::{Drop, FRAME_MAX, PAYLOAD_MAX};

#[test]
fn the_checksum_is_the_payload_byte_sum() {
    assert_eq!(wire::checksum(&[]), 0);
    assert_eq!(wire::checksum(&[0x01, 0x02, 0x03]), 6);
    // Two bytes that carry past the low byte carry into the high one.
    assert_eq!(wire::checksum(&[0xFF, 0x01]), 0x0100);
    // The sum wraps at 16 bits rather than widening.
    let full = vec![0xFFu8; PAYLOAD_MAX];
    assert_eq!(
        wire::checksum(&full),
        0xFFu16.wrapping_mul(PAYLOAD_MAX as u16)
    );
}

#[test]
fn a_prefix_is_one_byte_below_128_and_two_from_128() {
    assert_eq!(wire::prefix_size(0), 1);
    assert_eq!(wire::prefix_size(127), 1);
    assert_eq!(wire::prefix_size(128), 2);
    assert_eq!(wire::prefix_size(PAYLOAD_MAX), 2);
}

#[test]
fn encode_lays_out_the_prefix_the_payload_and_the_checksum() {
    let mut out = [0u8; FRAME_MAX];

    // One-byte prefix, then the payload, then the sum, low byte first.
    let used = wire::encode(&[0x01, 0x02, 0x03], &mut out).unwrap();
    assert_eq!(&out[..used], &[0x03, 0x01, 0x02, 0x03, 0x06, 0x00]);

    // An empty payload is three bytes: a zero length and a zero sum.
    let used = wire::encode(&[], &mut out).unwrap();
    assert_eq!(&out[..used], &[0x00, 0x00, 0x00]);

    // 127 stays one byte; 128 spills into a second with the continuation bit.
    let payload = vec![0x7Au8; 127];
    let used = wire::encode(&payload, &mut out).unwrap();
    assert_eq!(out[0], 0x7F);
    assert_eq!(used, 1 + 127 + 2);

    let payload = vec![0x7Au8; 128];
    let used = wire::encode(&payload, &mut out).unwrap();
    assert_eq!(&out[..2], &[0x80, 0x01]);
    assert_eq!(used, 2 + 128 + 2);

    let payload = vec![0x7Au8; PAYLOAD_MAX];
    let used = wire::encode(&payload, &mut out).unwrap();
    assert_eq!(&out[..2], &[0x80, 0x08]);
    assert_eq!(used, FRAME_MAX);

    // The sum lands in the last two bytes, low byte first.
    let payload = [0x00, 0x01, 0x00];
    let used = wire::encode(&payload, &mut out).unwrap();
    assert_eq!(&out[..used], &[0x03, 0x00, 0x01, 0x00, 0x01, 0x00]);
}

#[test]
fn encode_refuses_a_payload_above_the_maximum_and_a_short_buffer() {
    let payload = vec![0u8; PAYLOAD_MAX + 1];
    let mut out = [0u8; FRAME_MAX + 2];
    assert_eq!(wire::encode(&payload, &mut out), None);

    let mut out = [0u8; 4];
    assert_eq!(wire::encode(&[0x01, 0x02, 0x03], &mut out), None);
}

#[test]
fn every_length_that_fits_round_trips() {
    for length in [0usize, 1, 2, 126, 127, 128, 129, 512, PAYLOAD_MAX] {
        let payload: Vec<u8> = (0..length).map(|i| (i % 251) as u8).collect();
        let frame = wire::encode_to_vec(&payload).unwrap();
        assert!(frame.len() <= FRAME_MAX);

        let mut decoder = wire::Decoder::new();
        decoder.feed(&frame, 0);
        assert_eq!(decoder.next_frame().as_deref(), Some(payload.as_slice()));
        assert_eq!(decoder.next_frame(), None);
        assert_eq!(decoder.drops_all(), [0, 0, 0, 0]);
    }
}

#[test]
fn a_frame_split_across_feeds_is_read_the_same() {
    let payload: Vec<u8> = (0..900).map(|i| (i % 253) as u8).collect();
    let frame = wire::encode_to_vec(&payload).unwrap();

    for split in [1usize, 2, 127, 128, 131, frame.len() - 1] {
        let mut decoder = wire::Decoder::new();
        decoder.feed(&frame[..split], 0);
        assert_eq!(decoder.next_frame(), None, "split at {split} yielded early");
        decoder.feed(&frame[split..], 0);
        assert_eq!(decoder.next_frame().as_deref(), Some(payload.as_slice()));
    }
}

#[test]
fn a_bad_checksum_refuses_the_frame_whole_and_the_next_byte_is_a_prefix() {
    let mut decoder = wire::Decoder::new();
    // Length 3, three payload bytes, a sum that is not theirs.
    decoder.feed(&[0x03, 0x01, 0x02, 0x03, 0x07, 0x00], 0);
    assert_eq!(decoder.next_frame(), None);
    assert_eq!(decoder.drops(Drop::Checksum), 1);

    // A good frame straight after is read: the refusal lost no boundary.
    let good = wire::encode_to_vec(&[0x09]).unwrap();
    decoder.feed(&good, 0);
    assert_eq!(decoder.next_frame().as_deref(), Some(&[0x09u8][..]));
}

#[test]
fn a_declared_length_above_the_maximum_is_refused() {
    let mut decoder = wire::Decoder::new();
    // 0x81 0x08 declares 1025, one above the maximum.
    decoder.feed(&[0x81, 0x08], 0);
    assert_eq!(decoder.drops(Drop::Length), 1);
    assert_eq!(decoder.next_frame(), None);
}

#[test]
fn a_third_length_byte_is_refused() {
    let mut decoder = wire::Decoder::new();
    // Two bytes that both carry, then a third: no length that fits needs it.
    decoder.feed(&[0x80, 0x80, 0x80], 0);
    assert_eq!(decoder.drops(Drop::Prefix), 1);
}

#[test]
fn a_half_frame_is_dropped_after_the_idle_timeout() {
    let mut decoder = wire::Decoder::new();
    // A declared length of three, with one byte of it arrived.
    decoder.feed(&[0x03, 0x01], 0);
    assert!(!decoder.tick(wire::IDLE_TIMEOUT_MS - 1));
    assert!(decoder.tick(wire::IDLE_TIMEOUT_MS));
    assert_eq!(decoder.drops(Drop::Idle), 1);
}

#[test]
fn a_bare_continuation_prefix_is_not_half_a_frame() {
    let mut decoder = wire::Decoder::new();
    // A prefix that promised another byte declares nothing yet, so no timeout
    // can reach it.
    decoder.feed_byte(0x80, 0);
    assert!(!decoder.tick(10 * wire::IDLE_TIMEOUT_MS));
    assert_eq!(decoder.drops(Drop::Idle), 0);
}

#[test]
fn frames_are_held_until_they_are_taken() {
    let mut decoder = wire::Decoder::new();
    let first = wire::encode_to_vec(&[0x01]).unwrap();
    let second = wire::encode_to_vec(&[0x02]).unwrap();
    decoder.feed(&first, 0);
    decoder.feed(&second, 0);
    assert_eq!(decoder.queued(), 2);
    assert_eq!(decoder.next_frame().as_deref(), Some(&[0x01u8][..]));
    assert_eq!(decoder.next_frame().as_deref(), Some(&[0x02u8][..]));
    assert_eq!(decoder.next_frame(), None);
}
