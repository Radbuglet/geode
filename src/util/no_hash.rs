use std::{
	collections::HashSet,
	hash::{self, BuildHasherDefault},
	num::NonZeroU32,
};

// === NoOpHasher === //

pub type NoOpBuildHasher = BuildHasherDefault<NoOpHasher>;

#[derive(Debug, Clone, Default)]
pub struct NoOpHasher(u64);

impl hash::Hasher for NoOpHasher {
	fn write_u32(&mut self, i: u32) {
		debug_assert_eq!(self.0, 0);
		let i = i as u64;
		self.0 = (i << 32) + i;
	}

	fn write_u64(&mut self, i: u64) {
		debug_assert_eq!(self.0, 0);
		self.0 = i;
	}

	fn write(&mut self, _bytes: &[u8]) {
		unimplemented!("NoOpHasher only supports `write_u64` and `write_u32`.");
	}

	fn finish(&self) -> u64 {
		self.0
	}
}

// === RandIdGen === //

#[derive(Debug, Default)]
pub struct RandIdGen {
	rng: fastrand::Rng,
	ids: HashSet<NonZeroU32>,
}

impl RandIdGen {
	pub fn alloc(&mut self) -> NonZeroU32 {
		assert!(
			self.ids.len() < (u32::MAX / 2) as usize,
			"Allocated too many IDs"
		);

		// At at most half capacity, we are virtually guaranteed to find a random ID by chance very
		// quickly by just checking random IDs until we find one. Thus, while we could use a data
		// structure guaranteeing ID generation in a fixed number of steps, doing so would only increase
		// memory usage and introduce the opportunity for really subtle bugs. Naïve solution it is!
		loop {
			let Some(id) = NonZeroU32::new(self.rng.u32(..)) else {
				continue
			};

			if self.ids.insert(id) {
				break id;
			}
		}
	}

	pub fn dealloc(&mut self, id: NonZeroU32) {
		let removed = self.ids.remove(&id);
		debug_assert!(removed);
	}
}
