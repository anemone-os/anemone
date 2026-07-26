use crate::{
    prelude::*,
    task::files::{Fd, OpenedDescriptionCapability},
};

use super::watch::EpollWatch;

pub(super) const SLOT_COUNT: usize = MAX_FD_PER_PROCESS;
const SLOT_WORDS: usize = SLOT_COUNT / u64::BITS as usize;

static_assert!(
    SLOT_COUNT.is_multiple_of(u64::BITS as usize),
    "epoll slot capacity must fill the slot bitmaps"
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SlotId(usize);

impl SlotId {
    pub(super) const fn from_index(index: usize) -> Self {
        assert!(index < SLOT_COUNT, "epoll slot index out of range");
        Self(index)
    }

    pub(super) const fn index(self) -> usize {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SlotGeneration(u64);

#[derive(Debug, Clone, Copy)]
pub(super) struct SlotReservation {
    slot: SlotId,
    generation: SlotGeneration,
}

impl SlotReservation {
    pub(super) const fn slot(self) -> SlotId {
        self.slot
    }

    pub(super) const fn generation(self) -> SlotGeneration {
        self.generation
    }
}

enum SlotOccupant {
    Vacant,
    Reserved(SlotGeneration),
    Current(Arc<EpollWatch>),
}

struct SlotRecord {
    /// Monotonic allocator cursor, not the current watch identity. Failed ADD
    /// or MOD attempts may burn a generation so no stale callback can match a
    /// later publication after wraparound or rollback.
    last_generation: u64,
    occupant: SlotOccupant,
}

impl SlotRecord {
    const fn vacant() -> Self {
        Self {
            last_generation: 0,
            occupant: SlotOccupant::Vacant,
        }
    }

    fn next_generation(&mut self) -> Result<SlotGeneration, SysError> {
        let next = self
            .last_generation
            .checked_add(1)
            .ok_or(SysError::ResourceExhausted)?;
        self.last_generation = next;
        Ok(SlotGeneration(next))
    }
}

pub(super) struct WatchSlots {
    records: Vec<SlotRecord>,
}

impl WatchSlots {
    pub(super) fn try_new() -> Result<Self, SysError> {
        let mut records = Vec::new();
        records
            .try_reserve_exact(SLOT_COUNT)
            .map_err(|_| SysError::OutOfMemory)?;
        records.resize_with(SLOT_COUNT, SlotRecord::vacant);
        Ok(Self { records })
    }

    pub(super) fn reserve(&mut self) -> Result<SlotReservation, SysError> {
        let (index, record) = self
            .records
            .iter_mut()
            .enumerate()
            .find(|(_, record)| matches!(record.occupant, SlotOccupant::Vacant))
            .ok_or(SysError::ResourceExhausted)?;
        let generation = record.next_generation()?;
        record.occupant = SlotOccupant::Reserved(generation);
        Ok(SlotReservation {
            slot: SlotId(index),
            generation,
        })
    }

    pub(super) fn next_replacement_generation(
        &mut self,
        slot: SlotId,
    ) -> Result<SlotGeneration, SysError> {
        let record = &mut self.records[slot.index()];
        assert!(
            matches!(record.occupant, SlotOccupant::Current(_)),
            "epoll MOD generation reserved for non-current slot"
        );
        record.next_generation()
    }

    pub(super) fn publish_reserved(
        &mut self,
        reservation: SlotReservation,
        watch: Arc<EpollWatch>,
    ) {
        assert_eq!(watch.slot(), reservation.slot());
        assert_eq!(watch.generation(), reservation.generation());
        let occupant = &mut self.records[reservation.slot().index()].occupant;
        assert!(
            matches!(occupant, SlotOccupant::Reserved(generation) if *generation == reservation.generation()),
            "epoll ADD published an unreserved slot generation"
        );
        *occupant = SlotOccupant::Current(watch);
    }

    pub(super) fn release_reserved(&mut self, reservation: SlotReservation) {
        let occupant = &mut self.records[reservation.slot().index()].occupant;
        assert!(
            matches!(occupant, SlotOccupant::Reserved(generation) if *generation == reservation.generation()),
            "epoll ADD rollback lost its slot reservation"
        );
        *occupant = SlotOccupant::Vacant;
    }

    pub(super) fn replace_current(
        &mut self,
        slot: SlotId,
        replacement: Arc<EpollWatch>,
    ) -> Arc<EpollWatch> {
        assert_eq!(replacement.slot(), slot);
        let occupant = &mut self.records[slot.index()].occupant;
        let SlotOccupant::Current(current) = occupant else {
            panic!("epoll MOD replaced a non-current slot");
        };
        assert_ne!(current.generation(), replacement.generation());
        core::mem::replace(current, replacement)
    }

    pub(super) fn remove_current(&mut self, slot: SlotId) -> Arc<EpollWatch> {
        match core::mem::replace(
            &mut self.records[slot.index()].occupant,
            SlotOccupant::Vacant,
        ) {
            SlotOccupant::Current(watch) => watch,
            _ => panic!("epoll removed a non-current slot"),
        }
    }

    pub(super) fn current(&self, slot: SlotId) -> Option<Arc<EpollWatch>> {
        match &self.records[slot.index()].occupant {
            SlotOccupant::Current(watch) => Some(watch.clone()),
            SlotOccupant::Vacant | SlotOccupant::Reserved(_) => None,
        }
    }

    pub(super) fn find_current(
        &self,
        target: &OpenedDescriptionCapability,
        fd: Fd,
    ) -> Option<(SlotId, Arc<EpollWatch>)> {
        self.records
            .iter()
            .enumerate()
            .find_map(|(index, record)| match &record.occupant {
                SlotOccupant::Current(watch) if watch.same_key(target, fd) => {
                    Some((SlotId(index), watch.clone()))
                },
                _ => None,
            })
    }

    pub(super) fn retire_all(&mut self) {
        for record in &mut self.records {
            let occupant = core::mem::replace(&mut record.occupant, SlotOccupant::Vacant);
            match occupant {
                SlotOccupant::Vacant => {},
                SlotOccupant::Current(watch) => {
                    assert!(
                        watch.retire(),
                        "epoll teardown found a retired current watch"
                    );
                },
                SlotOccupant::Reserved(_) => {
                    panic!("epoll teardown observed an in-flight slot reservation")
                },
            }
        }
    }
}

#[derive(Clone, Copy)]
struct SlotSet {
    words: [u64; SLOT_WORDS],
}

impl SlotSet {
    const fn empty() -> Self {
        Self {
            words: [0; SLOT_WORDS],
        }
    }

    fn contains(&self, slot: SlotId) -> bool {
        let (word, bit) = Self::position(slot);
        self.words[word] & bit != 0
    }

    fn insert(&mut self, slot: SlotId) {
        let (word, bit) = Self::position(slot);
        self.words[word] |= bit;
    }

    fn remove(&mut self, slot: SlotId) {
        let (word, bit) = Self::position(slot);
        self.words[word] &= !bit;
    }

    fn clear(&mut self) {
        self.words.fill(0);
    }

    fn position(slot: SlotId) -> (usize, u64) {
        let word = slot.index() / u64::BITS as usize;
        let bit = slot.index() % u64::BITS as usize;
        (word, 1u64 << bit)
    }
}

/// Operation-owned candidate membership, one-shot disable state and fairness
/// cursor. None of these bits are target readiness truth.
pub(super) struct ReadySlots {
    candidates: SlotSet,
    disabled: SlotSet,
    cursor: usize,
}

impl ReadySlots {
    pub(super) const fn new() -> Self {
        Self {
            candidates: SlotSet::empty(),
            disabled: SlotSet::empty(),
            cursor: 0,
        }
    }

    pub(super) fn is_candidate(&self, slot: SlotId) -> bool {
        self.candidates.contains(slot)
    }

    pub(super) fn is_disabled(&self, slot: SlotId) -> bool {
        self.disabled.contains(slot)
    }

    pub(super) fn make_candidate(&mut self, slot: SlotId) {
        assert!(
            !self.is_disabled(slot),
            "disabled epoll watch became a ready candidate"
        );
        self.candidates.insert(slot);
    }

    pub(super) fn consume_candidate(&mut self, slot: SlotId) {
        self.candidates.remove(slot);
    }

    pub(super) fn disable(&mut self, slot: SlotId) {
        assert!(
            !self.is_candidate(slot),
            "epoll ONESHOT disabled a still-claimed candidate"
        );
        self.disabled.insert(slot);
    }

    pub(super) fn reset(&mut self, slot: SlotId) {
        self.candidates.remove(slot);
        self.disabled.remove(slot);
    }

    pub(super) fn reset_all(&mut self) {
        self.candidates.clear();
        self.disabled.clear();
        self.cursor = 0;
    }

    pub(super) fn claim_next(&mut self) -> Option<SlotId> {
        for offset in 0..SLOT_COUNT {
            let index = (self.cursor + offset) % SLOT_COUNT;
            let slot = SlotId(index);
            if self.is_candidate(slot) {
                self.consume_candidate(slot);
                self.cursor = (index + 1) % SLOT_COUNT;
                return Some(slot);
            }
        }
        None
    }
}

pub(super) struct DirtySnapshot {
    words: [u64; SLOT_WORDS],
}

impl DirtySnapshot {
    pub(super) fn slots(&self) -> impl Iterator<Item = SlotId> + '_ {
        self.words
            .iter()
            .enumerate()
            .flat_map(|(word_index, word)| {
                (0..u64::BITS as usize).filter_map(move |bit_index| {
                    (word & (1u64 << bit_index) != 0)
                        .then_some(SlotId(word_index * u64::BITS as usize + bit_index))
                })
            })
    }
}

pub(super) struct DirtySlots {
    words: [AtomicU64; SLOT_WORDS],
}

impl DirtySlots {
    pub(super) fn new() -> Self {
        Self {
            words: core::array::from_fn(|_| AtomicU64::new(0)),
        }
    }

    /// Publish a sticky recheck obligation without taking the operation mutex.
    /// The bit is a candidate hint only; generation and target readiness must
    /// be revalidated by the operation owner before it can affect behavior.
    pub(super) fn mark(&self, slot: SlotId) {
        let word = slot.index() / u64::BITS as usize;
        let bit = slot.index() % u64::BITS as usize;
        self.words[word].fetch_or(1u64 << bit, Ordering::Release);
    }

    /// Atomically claim the obligations visible at this point. Notifications
    /// racing after an individual swap remain set for the next refresh.
    pub(super) fn take(&self) -> DirtySnapshot {
        DirtySnapshot {
            words: core::array::from_fn(|index| self.words[index].swap(0, Ordering::AcqRel)),
        }
    }
}
