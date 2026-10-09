//! The content of leaf and branch pages, and the reference a leaf keeps to a
//! value stored in an overflow run.
//!
//! A leaf's content starts with a slot array: one 2-byte offset per entry, in
//! key order. The entries' cells sit at the end of the page, before the
//! check, in one of two layouts, which the page's kind names ([`Cells`]).
//! Format 5 wrote fixed lengths:
//!
//! | Size | Field                                             |
//! | ---- | ------------------------------------------------- |
//! | 2    | Key length `K`                                    |
//! | 1    | Value kind: 0 inline, 1 overflow                  |
//! | 2    | Inline only: value length `V`                     |
//! | `K`  | Key                                               |
//! | `V`  | Inline only: the value                            |
//! | 44   | Overflow only: the overflow reference             |
//!
//! Format 6 writes varint lengths, so that the short entries of an index,
//! whose cells were mostly lengths, take less:
//!
//! | Size  | Field                                                  |
//! | ----- | ------------------------------------------------------ |
//! | 1–2   | Key length `K`, a varint                               |
//! | 1–3   | A varint tag: `2·V` for an inline value of `V` bytes, 1 for an overflow reference |
//! | `K`   | Key                                                    |
//! | `V`   | Inline only: the value                                 |
//! | 44    | Overflow only: the overflow reference                  |
//!
//! A varint holds seven bits in each byte, the low ones first, and every byte
//! but the last has its top bit set. Neither layout changes the slots, the
//! longest key or the inline limit, so an entry that fits one fits the other,
//! and a cell of format 6 is never longer than the same cell of format 5.
//!
//! A branch with `k` keys holds `k + 1` child pointers, then a slot array of
//! `k` offsets, then the keys at the end of the page, each a 2-byte length and
//! the key. Child 0 holds the keys below key 0; child `i` holds the keys from
//! key `i − 1` up to key `i`; the last child holds the rest.
//!
//! Decoding validates every offset and length against the page, so a damaged
//! or crafted page produces an error, never a read outside it.

use std::cmp::Ordering;

use super::check::Check;
use super::page::{CONTENT_OFFSET, check_offset, content_len};
use super::pointer::{POINTER_LEN, Pointer};
use super::{le_u32, le_u64};

/// The size of an overflow reference, in bytes.
pub(crate) const OVERFLOW_REF_LEN: usize = 44;

/// The most bytes a leaf entry spends besides its key: slot, key length,
/// value kind and an overflow reference. The varint cells of format 6 spend
/// at most as much: two bytes of key length and one of tag.
const LEAF_OVERHEAD: usize = 2 + 2 + 1 + OVERFLOW_REF_LEN;

/// How a leaf lays out its cells: with the fixed lengths format 5 wrote, or
/// with the varint lengths of format 6. The page's kind names its layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cells {
    Fixed,
    Varint,
}

/// The bytes a varint of `value` takes.
const fn varint_len(value: usize) -> usize {
    if value < 1 << 7 {
        1
    } else if value < 1 << 14 {
        2
    } else {
        3
    }
}

/// Writes `value`, below 2^21, as a varint at offset `at`, and returns the
/// bytes it took.
#[expect(
    clippy::cast_possible_truncation,
    reason = "each byte takes seven bits of the value"
)]
fn write_varint(page: &mut [u8], at: usize, value: usize) -> usize {
    debug_assert!(value < 1 << 21);

    let len = varint_len(value);

    for index in 0..len {
        let more = if index + 1 < len { 0x80 } else { 0 };

        page[at + index] = (value >> (7 * index)) as u8 & 0x7f | more;
    }

    len
}

/// The varint at offset `at` of a page that was checked, and the bytes it
/// takes.
#[inline(always)]
fn read_varint(page: &[u8], at: usize) -> (usize, usize) {
    let first = page[at];

    if first < 0x80 {
        return (usize::from(first), 1);
    }

    let second = page[at + 1];
    let low = usize::from(first & 0x7f) | usize::from(second & 0x7f) << 7;

    if second < 0x80 {
        (low, 2)
    } else {
        (low | usize::from(page[at + 2]) << 14, 3)
    }
}

/// The varint at offset `at`, which has to end before `end`, take at most
/// three bytes, and be written in as few bytes as its value needs.
fn read_varint_checked(page: &[u8], at: usize, end: usize) -> Result<(usize, usize), &'static str> {
    let mut value = 0;

    for index in 0..3 {
        if at + index >= end {
            return Err("a leaf entry lies outside the page");
        }

        let byte = page[at + index];

        value |= usize::from(byte & 0x7f) << (7 * index);

        if byte < 0x80 {
            let len = index + 1;

            if len != varint_len(value) {
                return Err("a leaf entry's length is written in more bytes than it needs");
            }

            return Ok((value, len));
        }
    }

    Err("a leaf entry's length is longer than a page")
}

/// The header of a varint cell at offset `at` of a page that was checked:
/// the key's length, the tag, and where the key starts.
///
/// A key shorter than 128 bytes and a tag of one or two bytes, as nearly
/// every entry has, are read without a branch on how long the tag is: the
/// values of one tree straddle 64 bytes often enough that such a branch,
/// mispredicted, made a lookup 4% slower. The byte after a one-byte tag is
/// read and thrown away; it lies inside the page, since a cell ends before
/// the page's check.
#[inline(always)]
fn varint_header(page: &[u8], at: usize) -> (usize, usize, usize) {
    let first = page[at];
    let second = usize::from(page[at + 1]);
    let third = usize::from(page[at + 2]);
    let more = second >> 7;

    if first >= 0x80 || more & (third >> 7) != 0 {
        let (key_len, key_bytes) = read_varint(page, at);
        let (tag, tag_bytes) = read_varint(page, at + key_bytes);

        return (key_len, tag, at + key_bytes + tag_bytes);
    }

    // All ones when the tag has a second byte, and zero when it has not.
    let second_byte = 0usize.wrapping_sub(more);
    let tag = (second & 0x7f) | (((third & 0x7f) << 7) & second_byte);

    (usize::from(first), tag, at + 2 + more)
}

/// The longest key a page of `page_size` bytes allows: four entries of the
/// largest kind always fit in one node, so a node that overflows can always be
/// split into two valid ones.
pub(crate) fn max_key_len(page_size: usize) -> usize {
    (content_len(page_size) - 4 * LEAF_OVERHEAD) / 4
}

/// The largest leaf entry, slot included, that keeps its value inline. A
/// value that would make its entry larger goes into an overflow run.
pub(crate) fn inline_limit(page_size: usize) -> usize {
    content_len(page_size) / 4
}

/// The number of pages an overflow run of `len` bytes needs.
pub(crate) fn overflow_pages(page_size: usize, len: u64) -> u64 {
    len.div_ceil(content_len(page_size) as u64).max(1)
}

/// Where a value too large to keep inline is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OverflowRef {
    /// The first page of the run.
    pub(crate) first: u64,
    /// The commit that wrote the run.
    pub(crate) txn: u64,
    /// The number of pages in the run.
    pub(crate) pages: u32,
    /// The length of the value, in bytes.
    pub(crate) len: u64,
    /// XXH3-128 of the checks of the run's pages, concatenated in order.
    pub(crate) check: Check,
}

impl OverflowRef {
    fn write(&self, bytes: &mut [u8]) {
        bytes[0..8].copy_from_slice(&self.first.to_le_bytes());
        bytes[8..16].copy_from_slice(&self.txn.to_le_bytes());
        bytes[16..20].copy_from_slice(&self.pages.to_le_bytes());
        bytes[20..28].copy_from_slice(&self.len.to_le_bytes());
        self.check.write(&mut bytes[28..]);
    }

    fn read(bytes: &[u8], page_size: usize) -> Result<Self, &'static str> {
        let reference = Self {
            first: le_u64(bytes, 0),
            txn: le_u64(bytes, 8),
            pages: le_u32(bytes, 16),
            len: le_u64(bytes, 20),
            check: Check::read(&bytes[28..]),
        };

        if reference.first == 0 || reference.txn == 0 {
            return Err("an overflow reference points at no page");
        }

        if u64::from(reference.pages) != overflow_pages(page_size, reference.len) {
            return Err("an overflow reference's page count does not fit its length");
        }

        Ok(reference)
    }
}

/// A value as a leaf stores it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StoredValue {
    /// Kept in the leaf.
    Inline(Vec<u8>),
    /// Kept in an overflow run.
    Overflow(OverflowRef),
}

/// A value as a leaf stores it, read in place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StoredRef<'a> {
    Inline(&'a [u8]),
    Overflow(OverflowRef),
}

impl StoredValue {
    /// The value, borrowed.
    pub(crate) fn as_stored(&self) -> StoredRef<'_> {
        match self {
            StoredValue::Inline(value) => StoredRef::Inline(value),
            StoredValue::Overflow(reference) => StoredRef::Overflow(*reference),
        }
    }
}

/// One entry of a leaf.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LeafEntry {
    /// The key.
    pub(crate) key: Vec<u8>,
    /// The value, or where it is.
    pub(crate) value: StoredValue,
}

impl LeafEntry {
    /// The bytes this entry takes in a leaf of `cells`, slot included.
    pub(crate) fn len(&self, cells: Cells) -> usize {
        2 + cell_len(cells, self.key.len(), self.value.as_stored())
    }
}

/// The bytes an entry with an inline value takes in a leaf of `cells`, slot
/// included.
pub(crate) fn inline_entry_len(cells: Cells, key_len: usize, value_len: usize) -> usize {
    match cells {
        Cells::Fixed => 2 + 2 + 1 + 2 + key_len + value_len,
        Cells::Varint => 2 + varint_len(key_len) + varint_len(2 * value_len) + key_len + value_len,
    }
}

/// The bytes a branch with these keys takes, including one more child than
/// keys.
#[cfg(test)]
pub(crate) fn branch_len<K: AsRef<[u8]>>(keys: &[K]) -> usize {
    POINTER_LEN
        + keys
            .iter()
            .map(|key| branch_key_len(key.as_ref().len()))
            .sum::<usize>()
}

/// The bytes one key and the child after it take in a branch.
pub(crate) fn branch_key_len(key_len: usize) -> usize {
    POINTER_LEN + 2 + 2 + key_len
}

/// An offset inside a page, which is at most 65536 bytes.
#[expect(
    clippy::cast_possible_truncation,
    reason = "every offset lies inside a page of at most 65536 bytes"
)]
fn offset(value: usize) -> [u8; 2] {
    debug_assert!(value <= usize::from(u16::MAX));

    (value as u16).to_le_bytes()
}

fn read_u16(page: &[u8], at: usize) -> usize {
    usize::from(u16::from_le_bytes([page[at], page[at + 1]]))
}

/// The bytes an entry's cell takes in a leaf of `cells`, its slot not
/// included.
pub(crate) fn cell_len(cells: Cells, key_len: usize, value: StoredRef<'_>) -> usize {
    match (cells, value) {
        (_, StoredRef::Inline(value)) => inline_entry_len(cells, key_len, value.len()) - 2,
        (Cells::Fixed, StoredRef::Overflow(_)) => LEAF_OVERHEAD - 2 + key_len,
        (Cells::Varint, StoredRef::Overflow(_)) => {
            varint_len(key_len) + 1 + key_len + OVERFLOW_REF_LEN
        }
    }
}

/// Writes an entry's cell, of [`cell_len`] bytes, at offset `at` of a leaf
/// of `cells`.
pub(crate) fn write_cell(
    cells: Cells,
    page: &mut [u8],
    at: usize,
    key: &[u8],
    value: StoredRef<'_>,
) {
    let start = match (cells, value) {
        (Cells::Fixed, StoredRef::Inline(value)) => {
            page[at..at + 2].copy_from_slice(&offset(key.len()));
            page[at + 2] = 0;
            page[at + 3..at + 5].copy_from_slice(&offset(value.len()));
            at + 5
        }
        (Cells::Fixed, StoredRef::Overflow(_)) => {
            page[at..at + 2].copy_from_slice(&offset(key.len()));
            page[at + 2] = 1;
            at + 3
        }
        (Cells::Varint, StoredRef::Inline(value)) => {
            let key_bytes = write_varint(page, at, key.len());

            at + key_bytes + write_varint(page, at + key_bytes, 2 * value.len())
        }
        (Cells::Varint, StoredRef::Overflow(_)) => {
            let key_bytes = write_varint(page, at, key.len());

            at + key_bytes + write_varint(page, at + key_bytes, 1)
        }
    };

    page[start..start + key.len()].copy_from_slice(key);

    match value {
        StoredRef::Inline(value) => {
            page[start + key.len()..start + key.len() + value.len()].copy_from_slice(value);
        }
        StoredRef::Overflow(reference) => reference.write(&mut page[start + key.len()..]),
    }
}

/// Where the cell of entry `index` of a leaf of `cells` that [`check_leaf`]
/// passed lies, and how long it is.
pub(crate) fn leaf_cell(cells: Cells, page: &[u8], index: usize) -> (usize, usize) {
    let at = read_u16(page, CONTENT_OFFSET + 2 * index);
    let len = match cells {
        Cells::Fixed => {
            let key_len = read_u16(page, at);

            if page[at + 2] == 0 {
                inline_entry_len(cells, key_len, read_u16(page, at + 3)) - 2
            } else {
                LEAF_OVERHEAD - 2 + key_len
            }
        }
        Cells::Varint => {
            let (key_len, tag, start) = varint_header(page, at);

            start - at + key_len + if tag == 1 { OVERFLOW_REF_LEN } else { tag / 2 }
        }
    };

    (at, len)
}

/// Points slot `index` of a leaf at offset `at`.
pub(crate) fn set_slot(page: &mut [u8], index: usize, at: usize) {
    let slot = CONTENT_OFFSET + 2 * index;

    page[slot..slot + 2].copy_from_slice(&offset(at));
}

/// Writes `entries` as the content of a leaf of `cells`. The caller has
/// checked that they fit.
pub(crate) fn encode_leaf(cells: Cells, entries: &[LeafEntry], page: &mut [u8]) {
    let mut cursor = check_offset(page.len());

    for (index, entry) in entries.iter().enumerate() {
        let value = entry.value.as_stored();

        cursor -= cell_len(cells, entry.key.len(), value);
        write_cell(cells, page, cursor, &entry.key, value);
        set_slot(page, index, cursor);
    }

    debug_assert!(cursor >= CONTENT_OFFSET + 2 * entries.len());
}

/// Checks every offset, length and value kind of a leaf of `cells` with
/// `count` entries against the page, the order of its keys, and that no two
/// of its cells share a byte, so that [`leaf_key`] and [`leaf_value`] can
/// read it in place and a write transaction can change a cell without
/// changing another.
pub(crate) fn check_leaf(cells: Cells, page: &[u8], count: usize) -> Result<(), &'static str> {
    let page_size = page.len();
    let end = check_offset(page_size);
    let slots_end = CONTENT_OFFSET + 2 * count;
    let max_key = max_key_len(page_size);

    if count == 0 {
        return Err("a leaf holds no entry");
    }

    if slots_end > end {
        return Err("a leaf's slots run past its content");
    }

    let mut previous: Option<&[u8]> = None;
    // Where the cell before this one starts, for as long as every cell ends
    // at or below the start of the one before it, as they do in a page
    // written in slot order: then no two cells share a byte. Cells in any
    // other order are checked in a map of the page's bytes afterwards.
    let mut above = Some(end);

    for index in 0..count {
        let at = read_u16(page, CONTENT_OFFSET + 2 * index);

        if at < slots_end || at >= end {
            return Err("a leaf entry lies outside the page");
        }

        // Where the key starts, its length, and the length of the value, if
        // inline.
        let (key_start, key_len, inline) = match cells {
            Cells::Fixed if at + 3 > end => return Err("a leaf entry lies outside the page"),
            Cells::Fixed => match page[at + 2] {
                0 => {
                    if at + 5 > end {
                        return Err("a leaf entry lies outside the page");
                    }

                    (at + 5, read_u16(page, at), Some(read_u16(page, at + 3)))
                }
                1 => (at + 3, read_u16(page, at), None),
                _ => return Err("a leaf entry's value is of no known kind"),
            },
            Cells::Varint => {
                let (key_len, key_bytes) = read_varint_checked(page, at, end)?;
                let (tag, tag_bytes) = read_varint_checked(page, at + key_bytes, end)?;
                let key_start = at + key_bytes + tag_bytes;

                match tag {
                    1 => (key_start, key_len, None),
                    _ if tag % 2 == 0 => (key_start, key_len, Some(tag / 2)),
                    _ => return Err("a leaf entry's value is of no known kind"),
                }
            }
        };

        if key_len > max_key {
            return Err("a leaf key is longer than the page allows");
        }

        let cell_end = key_start + key_len + inline.unwrap_or(OVERFLOW_REF_LEN);

        if cell_end > end {
            return Err("a leaf entry lies outside the page");
        }

        if inline.is_none() {
            OverflowRef::read(&page[key_start + key_len..], page_size)?;
        }

        above = above.filter(|above| cell_end <= *above).map(|_| at);

        let key = &page[key_start..key_start + key_len];

        if previous.is_some_and(|previous| previous.cmp(key) != Ordering::Less) {
            return Err("a leaf's keys are out of order");
        }

        previous = Some(key);
    }

    match above {
        Some(_) => Ok(()),
        None => check_cells_apart(cells, page, count),
    }
}

/// Checks that no two cells of a leaf whose cells lie inside the page share
/// a byte, marking the bytes of each in a map of the page, one bit a byte.
fn check_cells_apart(cells: Cells, page: &[u8], count: usize) -> Result<(), &'static str> {
    let mut taken = vec![0u64; page.len().div_ceil(64)];

    for index in 0..count {
        let (mut at, len) = leaf_cell(cells, page, index);
        let cell_end = at + len;

        while at < cell_end {
            let (word, bit) = (at / 64, at % 64);
            let bits = (cell_end - at).min(64 - bit);
            let mask = (u64::MAX >> (64 - bits)) << bit;

            if taken[word] & mask != 0 {
                return Err("two leaf entries share bytes");
            }

            taken[word] |= mask;
            at += bits;
        }
    }

    Ok(())
}

/// The key of entry `index` of a leaf of `cells` that [`check_leaf`] passed.
#[inline(always)]
pub(crate) fn leaf_key(cells: Cells, page: &[u8], index: usize) -> &[u8] {
    let at = read_u16(page, CONTENT_OFFSET + 2 * index);

    match cells {
        Cells::Fixed => {
            let key_len = read_u16(page, at);
            let start = if page[at + 2] == 0 { at + 5 } else { at + 3 };

            &page[start..start + key_len]
        }
        Cells::Varint => {
            let (key_len, _, start) = varint_header(page, at);

            &page[start..start + key_len]
        }
    }
}

/// The value of entry `index` of a leaf of `cells` that [`check_leaf`]
/// passed.
pub(crate) fn leaf_value(
    cells: Cells,
    page: &[u8],
    index: usize,
) -> Result<StoredRef<'_>, &'static str> {
    leaf_entry(cells, page, index).map(|(_, value)| value)
}

/// The key and the value of entry `index` of a leaf of `cells` that
/// [`check_leaf`] passed, if the value is inline: nearly every value, in a
/// return small enough to pass in registers.
#[inline(always)]
pub(crate) fn leaf_inline(cells: Cells, page: &[u8], index: usize) -> Option<(&[u8], &[u8])> {
    let at = read_u16(page, CONTENT_OFFSET + 2 * index);
    let (key_start, key_len, value_len) = match cells {
        Cells::Fixed => {
            if page[at + 2] != 0 {
                return None;
            }

            (at + 5, read_u16(page, at), read_u16(page, at + 3))
        }
        Cells::Varint => {
            let (key_len, tag, start) = varint_header(page, at);

            if tag == 1 {
                return None;
            }

            (start, key_len, tag / 2)
        }
    };
    let key_end = key_start + key_len;

    Some((
        &page[key_start..key_end],
        &page[key_end..key_end + value_len],
    ))
}

/// The key and the value of entry `index` of a leaf of `cells` that
/// [`check_leaf`] passed, read together.
pub(crate) fn leaf_entry(
    cells: Cells,
    page: &[u8],
    index: usize,
) -> Result<(&[u8], StoredRef<'_>), &'static str> {
    if let Some((key, value)) = leaf_inline(cells, page, index) {
        return Ok((key, StoredRef::Inline(value)));
    }

    let key = leaf_key(cells, page, index);
    // The key's slice ends where the reference starts.
    let start = key.as_ptr() as usize - page.as_ptr() as usize + key.len();

    Ok((
        key,
        StoredRef::Overflow(OverflowRef::read(&page[start..], page.len())?),
    ))
}

/// The bytes the `count` entries of a leaf of `cells` that [`check_leaf`]
/// passed take, slots included, as [`LeafEntry::len`] counts them, and where
/// its lowest cell starts: the end of its free space, `check_offset` of the
/// page when it has no entry.
pub(crate) fn leaf_extent(cells: Cells, page: &[u8], count: usize) -> (usize, usize) {
    (0..count).fold((0, check_offset(page.len())), |(size, low), index| {
        let (at, len) = leaf_cell(cells, page, index);

        (size + 2 + len, low.min(at))
    })
}

/// Rewrites a leaf of `count` entries laid out in `from` cells into a page
/// of `to` cells, entries and slots in the same order. A cell of format 6 is
/// never longer than the same cell of format 5, so a leaf of format 5 always
/// fits in a page of format 6. Returns the new page.
pub(crate) fn convert_leaf(
    from: Cells,
    to: Cells,
    page: &[u8],
    count: usize,
) -> Result<Vec<u8>, &'static str> {
    let mut converted = vec![0; page.len()];
    let mut cursor = check_offset(page.len());

    for index in 0..count {
        let (key, value) = leaf_entry(from, page, index)?;
        let len = cell_len(to, key.len(), value);

        if cursor < CONTENT_OFFSET + 2 * count + len {
            return Err("a leaf does not fit in a page of the other layout");
        }

        cursor -= len;
        write_cell(to, &mut converted, cursor, key, value);
        set_slot(&mut converted, index, cursor);
    }

    converted[..CONTENT_OFFSET].copy_from_slice(&page[..CONTENT_OFFSET]);

    Ok(converted)
}

/// Where the lowest cell of a leaf with `count` entries starts, read from
/// its slots alone: the end of its free space, `check_offset` of the page
/// when it has no entry.
pub(crate) fn leaf_low(page: &[u8], count: usize) -> usize {
    page[CONTENT_OFFSET..CONTENT_OFFSET + 2 * count]
        .chunks_exact(2)
        .map(|slot| usize::from(u16::from_le_bytes([slot[0], slot[1]])))
        .min()
        .unwrap_or(check_offset(page.len()))
}

/// Reads the `count` entries of a leaf.
#[cfg(test)]
pub(crate) fn decode_leaf(
    cells: Cells,
    page: &[u8],
    count: usize,
) -> Result<Vec<LeafEntry>, &'static str> {
    check_leaf(cells, page, count)?;

    (0..count)
        .map(|index| {
            Ok(LeafEntry {
                key: leaf_key(cells, page, index).to_vec(),
                value: match leaf_value(cells, page, index)? {
                    StoredRef::Inline(value) => StoredValue::Inline(value.to_vec()),
                    StoredRef::Overflow(reference) => StoredValue::Overflow(reference),
                },
            })
        })
        .collect()
}

/// Writes a branch's keys and children as its content. There is one more
/// child than keys, and the caller has checked that they fit.
pub(crate) fn encode_branch<K: AsRef<[u8]>>(
    keys: impl IntoIterator<Item = K>,
    children: &[Pointer],
    page: &mut [u8],
) {
    write_branch_children(children, page);

    let slots = CONTENT_OFFSET + POINTER_LEN * children.len();
    let mut cursor = check_offset(page.len());
    let mut count = 0;

    for (index, key) in keys.into_iter().enumerate() {
        let key = key.as_ref();

        cursor -= 2 + key.len();
        page[cursor..cursor + 2].copy_from_slice(&offset(key.len()));
        page[cursor + 2..cursor + 2 + key.len()].copy_from_slice(key);

        let slot = slots + 2 * index;

        page[slot..slot + 2].copy_from_slice(&offset(cursor));
        count += 1;
    }

    debug_assert_eq!(children.len(), count + 1);
    debug_assert!(cursor >= slots + 2 * count);
}

/// Writes the pointers to a branch's children into `page`, which holds the
/// branch's keys already, or is about to.
pub(crate) fn write_branch_children(children: &[Pointer], page: &mut [u8]) {
    for (index, child) in children.iter().enumerate() {
        child.write(&mut page[CONTENT_OFFSET + POINTER_LEN * index..]);
    }
}

/// Checks the children, the key offsets and lengths of a branch with
/// `count` keys against the page, and the order of its keys, so that
/// [`branch_key`] and [`branch_child`] can read it in place.
pub(crate) fn check_branch(page: &[u8], count: usize) -> Result<(), &'static str> {
    let end = check_offset(page.len());
    let slots = CONTENT_OFFSET + POINTER_LEN * (count + 1);
    let slots_end = slots + 2 * count;
    let max_key = max_key_len(page.len());

    if count == 0 {
        return Err("a branch holds no key");
    }

    if slots_end > end {
        return Err("a branch's children run past its content");
    }

    for index in 0..=count {
        let child = branch_child(page, index);

        if child.page == 0 || child.txn == 0 {
            return Err("a branch points at no page");
        }
    }

    let mut previous: Option<&[u8]> = None;

    for index in 0..count {
        let at = read_u16(page, slots + 2 * index);

        if at < slots_end || at + 2 > end {
            return Err("a branch key lies outside the page");
        }

        let key_len = read_u16(page, at);

        if key_len > max_key || at + 2 + key_len > end {
            return Err("a branch key lies outside the page");
        }

        let key = &page[at + 2..at + 2 + key_len];

        if previous.is_some_and(|previous| previous >= key) {
            return Err("a branch's keys are out of order");
        }

        previous = Some(key);
    }

    Ok(())
}

/// Key `index` of a branch with `count` keys that [`check_branch`] passed.
pub(crate) fn branch_key(page: &[u8], count: usize, index: usize) -> &[u8] {
    let at = read_u16(page, CONTENT_OFFSET + POINTER_LEN * (count + 1) + 2 * index);

    &page[at + 2..at + 2 + read_u16(page, at)]
}

/// Child `index` of a branch.
pub(crate) fn branch_child(page: &[u8], index: usize) -> Pointer {
    Pointer::read(&page[CONTENT_OFFSET + POINTER_LEN * index..])
}

/// The bytes a branch with `count` keys that [`check_branch`] passed takes,
/// as [`branch_len`] counts them.
pub(crate) fn branch_size(page: &[u8], count: usize) -> usize {
    POINTER_LEN
        + (0..count)
            .map(|index| branch_key_len(branch_key(page, count, index).len()))
            .sum::<usize>()
}

/// Reads the `count` keys and `count + 1` children of a branch.
#[cfg(test)]
pub(crate) fn decode_branch(
    page: &[u8],
    count: usize,
) -> Result<(Vec<Vec<u8>>, Vec<Pointer>), &'static str> {
    check_branch(page, count)?;

    Ok((
        (0..count)
            .map(|index| branch_key(page, count, index).to_vec())
            .collect(),
        (0..=count).map(|index| branch_child(page, index)).collect(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const P: usize = 4096;

    /// Both layouts of a leaf's cells, which every test of leaves runs with.
    const LAYOUTS: [Cells; 2] = [Cells::Fixed, Cells::Varint];

    /// The length of the value of `key` whose inline cell takes `len` bytes
    /// in `cells`.
    fn value_len_for(cells: Cells, key: &[u8], len: usize) -> usize {
        (0..len)
            .rev()
            .find(|value| inline_entry_len(cells, key.len(), *value) - 2 == len)
            .unwrap()
    }

    fn inline(key: &[u8], value: &[u8]) -> LeafEntry {
        LeafEntry {
            key: key.to_vec(),
            value: StoredValue::Inline(value.to_vec()),
        }
    }

    fn reference() -> OverflowRef {
        OverflowRef {
            first: 9,
            txn: 3,
            pages: 2,
            len: 5000,
            check: Check::of(&[b"run"]),
        }
    }

    #[test]
    fn the_limits_are_the_documented_ones() {
        assert_eq!(max_key_len(4096), 957);
        assert_eq!(max_key_len(16384), 4029);
        assert_eq!(max_key_len(65536), 16317);
        assert_eq!(inline_limit(4096), 1006);
        assert_eq!(overflow_pages(4096, 4024), 1);
        assert_eq!(overflow_pages(4096, 4025), 2);
    }

    #[test]
    fn four_of_the_largest_entries_always_fit() {
        for page_size in [4096, 16384, 65536] {
            let key = max_key_len(page_size);
            let leaf = 4 * (LEAF_OVERHEAD + key);
            let branch = branch_len(&vec![vec![0u8; key]; 4]);

            assert!(leaf <= content_len(page_size), "{page_size}");
            assert!(branch <= content_len(page_size), "{page_size}");
        }
    }

    #[test]
    fn a_leaf_reads_back_as_it_was_written() {
        for cells in LAYOUTS {
            let entries = vec![
                inline(b"", b"the empty key"),
                inline(b"apple", b""),
                LeafEntry {
                    key: b"banana".to_vec(),
                    value: StoredValue::Overflow(reference()),
                },
                inline(b"cherry", &[0xAB; 300]),
                inline(&[b'k'; 200], &[0xCD; 64]),
            ];
            let mut page = vec![0u8; P];

            encode_leaf(cells, &entries, &mut page);

            assert_eq!(decode_leaf(cells, &page, entries.len()), Ok(entries));
        }
    }

    /// Lengths on either side of a varint's byte boundaries read back as
    /// they were written, in pages large enough for them.
    #[test]
    fn varint_lengths_read_back_on_either_side_of_their_boundaries() {
        let page_size = 65536;

        for key_len in [0, 1, 127, 128, 900, 16_317] {
            for value_len in [0, 63, 64, 8191, 8192] {
                let entry = inline(&vec![b'k'; key_len], &vec![0x5A; value_len]);

                if entry.len(Cells::Varint) > inline_limit(page_size) && value_len > 0 {
                    continue;
                }

                let mut page = vec![0u8; page_size];

                encode_leaf(Cells::Varint, std::slice::from_ref(&entry), &mut page);

                assert_eq!(
                    decode_leaf(Cells::Varint, &page, 1),
                    Ok(vec![entry.clone()]),
                    "key {key_len}, value {value_len}"
                );
                assert_eq!(
                    leaf_cell(Cells::Varint, &page, 0).1,
                    cell_len(Cells::Varint, key_len, entry.value.as_stored())
                );
            }
        }
    }

    #[test]
    fn a_leaf_entry_has_the_documented_layout() {
        let mut page = vec![0u8; P];

        encode_leaf(Cells::Fixed, &[inline(b"ab", b"xyz")], &mut page);

        let at = read_u16(&page, CONTENT_OFFSET);

        assert_eq!(at, check_offset(P) - 10, "packed against the end");
        assert_eq!(
            &page[at..at + 10],
            &[2, 0, 0, 3, 0, b'a', b'b', b'x', b'y', b'z']
        );

        encode_leaf(Cells::Varint, &[inline(b"ab", b"xyz")], &mut page);

        let at = read_u16(&page, CONTENT_OFFSET);

        assert_eq!(at, check_offset(P) - 7, "packed against the end");
        assert_eq!(&page[at..at + 7], &[2, 6, b'a', b'b', b'x', b'y', b'z']);

        // A value of 100 bytes is a tag of 200, in two bytes, and an
        // overflow reference a tag of 1.
        encode_leaf(
            Cells::Varint,
            &[
                inline(b"a", &[0; 100]),
                LeafEntry {
                    key: b"b".to_vec(),
                    value: StoredValue::Overflow(reference()),
                },
            ],
            &mut page,
        );

        let at = read_u16(&page, CONTENT_OFFSET);

        assert_eq!(&page[at..at + 4], &[1, 0xC8, 0x01, b'a']);

        let at = read_u16(&page, CONTENT_OFFSET + 2);

        assert_eq!(&page[at..at + 3], &[1, 1, b'b']);
    }

    /// A cell of format 6 is never longer than the same cell of format 5,
    /// for every key and value a page of any size can hold, so a leaf of
    /// format 5 always fits in a page of format 6.
    #[test]
    fn a_varint_cell_is_never_longer_than_a_fixed_one() {
        let reference = reference();

        for key_len in 0..=max_key_len(65536) {
            for value_len in [0, 1, 63, 64, 127, 128, 8191, 8192, inline_limit(65536)] {
                let value = vec![0; value_len];

                assert!(
                    cell_len(Cells::Varint, key_len, StoredRef::Inline(&value))
                        <= cell_len(Cells::Fixed, key_len, StoredRef::Inline(&value))
                );
            }

            assert!(
                cell_len(Cells::Varint, key_len, StoredRef::Overflow(reference))
                    <= cell_len(Cells::Fixed, key_len, StoredRef::Overflow(reference))
            );
        }
    }

    #[test]
    fn a_leaf_converts_from_one_layout_to_the_other() {
        let entries = vec![
            inline(b"a", b"one"),
            LeafEntry {
                key: b"b".to_vec(),
                value: StoredValue::Overflow(reference()),
            },
            inline(b"c", &[7; 500]),
        ];
        let mut page = vec![0u8; P];

        encode_leaf(Cells::Fixed, &entries, &mut page);

        let converted = convert_leaf(Cells::Fixed, Cells::Varint, &page, 3).unwrap();

        assert_eq!(
            decode_leaf(Cells::Varint, &converted, 3),
            Ok(entries.clone())
        );

        let back = convert_leaf(Cells::Varint, Cells::Fixed, &converted, 3).unwrap();

        assert_eq!(back, page);
    }

    /// A varint written in more bytes than its value needs, or one that runs
    /// on past three bytes, is refused, so that each entry has one encoding,
    /// as a fixed length has; so is a tag that is odd and not 1.
    #[test]
    fn varints_in_more_bytes_than_needed_and_odd_tags_are_refused() {
        let end = check_offset(P);
        let at = end - 8;
        let cell = |bytes: &[u8]| {
            let mut page = vec![0u8; P];

            page[at..at + bytes.len()].copy_from_slice(bytes);
            set_slot(&mut page, 0, at);

            page
        };

        assert!(check_leaf(Cells::Varint, &cell(&[1, 4, b'a', 1, 2]), 1).is_ok());
        assert!(check_leaf(Cells::Varint, &cell(&[0x81, 0, 4, b'a', 1, 2]), 1).is_err());
        assert!(check_leaf(Cells::Varint, &cell(&[1, 0x84, 0, b'a', 1, 2]), 1).is_err());
        assert!(check_leaf(Cells::Varint, &cell(&[0x81, 0x80, 0x80, 0x80]), 1).is_err());
        assert!(check_leaf(Cells::Varint, &cell(&[1, 3, b'a', 1, 2]), 1).is_err());
    }

    #[test]
    fn a_branch_reads_back_as_it_was_written() {
        let keys = vec![b"m".to_vec(), b"t".to_vec()];
        let children: Vec<Pointer> = (1..=3)
            .map(|page| Pointer {
                page,
                txn: 2,
                check: Check::of(&[&page.to_le_bytes()]),
            })
            .collect();
        let mut page = vec![0u8; P];

        encode_branch(&keys, &children, &mut page);

        assert_eq!(decode_branch(&page, 2), Ok((keys, children)));
        assert_eq!(
            Pointer::read(&page[CONTENT_OFFSET..]).page,
            1,
            "child 0 first"
        );
    }

    #[test]
    fn keys_out_of_order_are_refused() {
        for cells in LAYOUTS {
            let mut page = vec![0u8; P];

            encode_leaf(cells, &[inline(b"b", b""), inline(b"a", b"")], &mut page);

            assert!(decode_leaf(cells, &page, 2).is_err());

            encode_leaf(cells, &[inline(b"a", b""), inline(b"a", b"")], &mut page);

            assert!(decode_leaf(cells, &page, 2).is_err(), "a key twice");
        }
    }

    /// A leaf whose cells share bytes is refused, and one whose cells lie
    /// apart in any order is read. The cells that overlap here read back as
    /// themselves, so that nothing else about the page is wrong.
    #[test]
    fn leaf_cells_that_share_bytes_are_refused() {
        for layout in LAYOUTS {
            cells_that_share_bytes_are_refused(layout);
        }
    }

    fn cells_that_share_bytes_are_refused(layout: Cells) {
        let end = check_offset(P);
        let decode_leaf = |page: &[u8], count| decode_leaf(layout, page, count);
        // Cells of `len` bytes at the offsets given, written in that order,
        // with keys in slot order and values of 0xEE ending in `last`.
        let leaf = |cells: &[(usize, &[u8], usize, u8)], slots: &[usize]| {
            let mut page = vec![0u8; P];

            for &(at, key, len, last) in cells {
                let mut value = vec![0xEE; value_len_for(layout, key, len)];

                *value.last_mut().unwrap() = last;
                write_cell(layout, &mut page, at, key, StoredRef::Inline(&value));
            }

            for (index, &at) in slots.iter().enumerate() {
                set_slot(&mut page, index, at);
            }

            page
        };

        // Apart, in slot order and out of it.
        let apart = [(end - 10, &b"a"[..], 10, 0), (end - 30, b"b", 10, 0)];

        assert!(decode_leaf(&leaf(&apart, &[end - 10, end - 30]), 2).is_ok());

        let apart = [(end - 30, &b"a"[..], 10, 0), (end - 10, b"b", 10, 0)];

        assert!(decode_leaf(&leaf(&apart, &[end - 30, end - 10]), 2).is_ok());

        // A cell inside another's value, after it in slot order and before it.
        let inside = [(end - 40, &b"a"[..], 40, 0), (end - 20, b"b", 18, 0)];

        assert!(decode_leaf(&leaf(&inside, &[end - 40, end - 20]), 2).is_err());

        let inside = [(end - 40, &b"b"[..], 40, 0), (end - 20, b"a", 18, 0)];

        assert!(decode_leaf(&leaf(&inside, &[end - 20, end - 40]), 2).is_err());

        // A cell whose last byte, 1, is the first of the next cell's key
        // length, which is 1.
        let touching = [(end - 10, &b"a"[..], 10, 0), (end - 19, b"b", 10, 1)];

        assert!(decode_leaf(&leaf(&touching, &[end - 10, end - 19]), 2).is_err());
    }

    #[test]
    fn offsets_outside_the_page_are_refused_rather_than_followed() {
        for cells in LAYOUTS {
            let mut page = vec![0u8; P];

            encode_leaf(cells, &[inline(b"key", b"value")], &mut page);

            // A slot that points into the check, and a key length that runs
            // out.
            page[CONTENT_OFFSET..CONTENT_OFFSET + 2].copy_from_slice(&offset(P - 8));

            assert!(decode_leaf(cells, &page, 1).is_err());

            encode_leaf(cells, &[inline(b"key", b"value")], &mut page);

            let at = read_u16(&page, CONTENT_OFFSET);

            // A key 900 bytes long, in either layout.
            match cells {
                Cells::Fixed => page[at..at + 2].copy_from_slice(&offset(900)),
                Cells::Varint => {
                    let mut moved = vec![0u8; P];

                    moved[at - 1..at + 1].copy_from_slice(&[0x84, 0x07]);
                    moved[at + 1..check_offset(P)].copy_from_slice(&page[at + 1..check_offset(P)]);
                    set_slot(&mut moved, 0, at - 1);
                    page = moved;
                }
            }

            assert!(decode_leaf(cells, &page, 1).is_err());
            assert!(
                decode_leaf(cells, &page, 3000).is_err(),
                "more slots than the page holds"
            );
            assert!(decode_leaf(cells, &page, 0).is_err(), "an empty leaf");
        }
    }

    #[test]
    fn a_branch_pointing_at_page_zero_is_refused() {
        let mut page = vec![0u8; P];

        encode_branch([b"k"], &[Pointer::NULL, Pointer::NULL], &mut page);

        assert!(decode_branch(&page, 1).is_err());
    }

    #[test]
    fn an_overflow_reference_whose_length_and_pages_disagree_is_refused() {
        let mut bad = reference();

        bad.pages = 3;

        for cells in LAYOUTS {
            let mut page = vec![0u8; P];

            encode_leaf(
                cells,
                &[LeafEntry {
                    key: b"k".to_vec(),
                    value: StoredValue::Overflow(bad),
                }],
                &mut page,
            );

            assert!(decode_leaf(cells, &page, 1).is_err());
        }
    }
}
