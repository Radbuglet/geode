use std::ops::{Index, IndexMut};

use derive_where::derive_where;
use hibitset::BitSetLike;

fn slot_to_usize(v: u32) -> usize {
	usize::try_from(v).unwrap()
}

#[derive(Debug, Clone)]
#[derive_where(Default)]
pub struct FreeList<T> {
	slots: Vec<Option<T>>,
	free: hibitset::BitSet,
}

impl<T> FreeList<T> {
	pub fn alloc(&mut self, value: T) -> u32 {
		match (&self.free).iter().next() {
			Some(slot) => {
				self.free.remove(slot);
				self.slots[slot_to_usize(slot)] = Some(value);
				slot
			}
			None => {
				let slot = u32::try_from(self.slots.len()).unwrap();
				self.slots.push(Some(value));
				self.free.add(slot);
				slot
			}
		}
	}

	pub fn dealloc(&mut self, slot: u32) -> Option<T> {
		self.free.add(slot);
		self.slots[slot_to_usize(slot)].take()
	}

	pub fn get(&self, slot: u32) -> Option<&T> {
		match self.slots.get(slot_to_usize(slot)) {
			Some(Some(v)) => Some(v),
			_ => None,
		}
	}

	pub fn get_mut(&mut self, slot: u32) -> Option<&mut T> {
		match self.slots.get_mut(slot_to_usize(slot)) {
			Some(Some(v)) => Some(v),
			_ => None,
		}
	}

	// pub fn as_slice(&self) -> &[Option<T>] {
	// 	&self.slots
	// }

	// pub fn as_slice_mut(&mut self) -> &mut [Option<T>] {
	// 	&mut self.slots
	// }
}

impl<T> Index<u32> for FreeList<T> {
	type Output = T;

	fn index(&self, slot: u32) -> &Self::Output {
		self.get(slot).unwrap()
	}
}

impl<T> IndexMut<u32> for FreeList<T> {
	fn index_mut(&mut self, slot: u32) -> &mut Self::Output {
		self.get_mut(slot).unwrap()
	}
}
