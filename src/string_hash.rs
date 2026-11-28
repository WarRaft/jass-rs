//! The game's `StringHash`: Storm's `SStrHash2` (as pjass has it from the
//! game): Bob Jenkins' lookup2 hash (the 1997 Dr Dobb's article), seed 0,
//! of the text with `a`-`z` made `A`-`Z` and `/` made `\\`. Shared by the
//! lint (`string-hash-collision`) and a build (`wc3-build`'s
//! `string_hash`, which works it out as the map is built).

/// lookup2's mix of three words.
fn mix(a: &mut u32, b: &mut u32, c: &mut u32) {
    *a = a.wrapping_sub(*b).wrapping_sub(*c) ^ (*c >> 13);
    *b = b.wrapping_sub(*c).wrapping_sub(*a) ^ (*a << 8);
    *c = c.wrapping_sub(*a).wrapping_sub(*b) ^ (*b >> 13);
    *a = a.wrapping_sub(*b).wrapping_sub(*c) ^ (*c >> 12);
    *b = b.wrapping_sub(*c).wrapping_sub(*a) ^ (*a << 16);
    *c = c.wrapping_sub(*a).wrapping_sub(*b) ^ (*b >> 5);
    *a = a.wrapping_sub(*b).wrapping_sub(*c) ^ (*c >> 3);
    *b = b.wrapping_sub(*c).wrapping_sub(*a) ^ (*a << 10);
    *c = c.wrapping_sub(*a).wrapping_sub(*b) ^ (*b >> 15);
}

/// Bob Jenkins' lookup2 `hash(k, length, initval)`.
pub fn lookup2(k: &[u8], initval: u32) -> u32 {
    let word = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let (mut a, mut b, mut c) = (0x9e37_79b9u32, 0x9e37_79b9u32, initval);
    let mut rest = k;
    while rest.len() >= 12 {
        a = a.wrapping_add(word(&rest[0..4]));
        b = b.wrapping_add(word(&rest[4..8]));
        c = c.wrapping_add(word(&rest[8..12]));
        mix(&mut a, &mut b, &mut c);
        rest = &rest[12..];
    }
    // The last 11 bytes at most: c's lowest byte is the length's.
    c = c.wrapping_add(k.len() as u32);
    for (i, &byte) in rest.iter().enumerate() {
        let v = byte as u32;
        match i {
            0..=3 => a = a.wrapping_add(v << (8 * i)),
            4..=7 => b = b.wrapping_add(v << (8 * (i - 4))),
            _ => c = c.wrapping_add(v << (8 * (i - 7))),
        }
    }
    mix(&mut a, &mut b, &mut c);
    c
}

/// What the game's `StringHash` gives for `text` (ASCII).
pub fn string_hash(text: &[u8]) -> i32 {
    let normal: Vec<u8> = text
        .iter()
        .map(|&b| match b {
            b'a'..=b'z' => b - 0x20,
            b'/' => b'\\',
            b => b,
        })
        .collect();
    lookup2(&normal, 0) as i32
}

