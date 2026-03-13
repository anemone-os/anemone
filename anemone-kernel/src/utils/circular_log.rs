#[derive(Debug, Clone)]
struct Slot<T> {
    item: Option<T>,
    generation: usize,
}

#[derive(Debug, Clone)]
pub struct CircularLog<T, const N: usize> {
    buffer: [Slot<T>; N],
    head_seq: usize,
}

#[derive(Debug)]
pub enum ReadErr {
    Overwritten,
    NotReached,
}

impl<T, const N: usize> CircularLog<T, N> {
    pub const fn new() -> Self {
        Self {
            buffer: [const {
                Slot {
                    item: None,
                    generation: 0,
                }
            }; N],
            head_seq: 0,
        }
    }

    pub fn push(&mut self, item: T) {
        let idx = self.head_seq % N;
        let generation = self.head_seq / N;
        self.buffer[idx] = Slot {
            item: Some(item),
            generation,
        };
        self.head_seq += 1;
    }

    pub fn get_at(&self, seq: usize) -> Result<T, ReadErr>
    where
        T: Clone,
    {
        if seq >= self.head_seq {
            return Err(ReadErr::NotReached);
        }

        let idx = seq % N;
        let generation = seq / N;

        let slot = &self.buffer[idx];
        if slot.generation > generation {
            Err(ReadErr::Overwritten)
        } else if slot.generation == generation {
            Ok(slot
                .item
                .as_ref()
                .expect("generation matches but item is None")
                .clone())
        } else {
            unreachable!("generation should never be less than the expected value");
        }
    }

    pub fn head_seq(&self) -> usize {
        self.head_seq
    }

    pub fn oldest_seq(&self) -> usize {
        self.head_seq.saturating_sub(N)
    }
}
