// TODO: Implement `TransOption<T>` and re-introduce to `Storage<T>`

/*
use std::{
	alloc::Layout,
	borrow::Borrow,
	collections::HashMap,
	fmt,
	hash::{BuildHasher, Hash},
	marker::PhantomData,
	mem::{self, ManuallyDrop},
	slice,
};

use crate::util::ptr::PointeeCastExt;

// === InlineStore === //

// FIXME: Be aware of and maybe address https://github.com/rust-lang/rust/issues/99604
#[repr(C)]
pub union InlineStore<C> {
	zst: (),
	_placeholder: ManuallyDrop<C>,
}

impl<C> InlineStore<C> {
	pub const fn can_hold_layout(layout: Layout) -> bool {
		// Alignment
		mem::align_of::<C>() >= layout.align()
			// Size
			&& mem::size_of::<C>() >= layout.size()
	}

	pub const fn can_hold<T>() -> bool {
		Self::can_hold_layout(Layout::new::<T>())
	}

	pub fn try_new<T>(value: T) -> Result<Self, T> {
		if Self::can_hold::<T>() {
			let mut target = Self { zst: () };

			unsafe { (&mut target as *mut Self).cast::<T>().write(value) };

			Ok(target)
		} else {
			Err(value)
		}
	}

	pub fn new<T>(value: T) -> Self {
		Self::try_new(value).ok().unwrap()
	}

	pub fn as_ptr<T>(&self) -> *const T {
		assert!(Self::can_hold::<T>());

		(self as *const Self).cast::<T>()
	}

	pub fn as_ptr_mut<T>(&mut self) -> *mut T {
		assert!(Self::can_hold::<T>());

		(self as *mut Self).cast::<T>()
	}

	pub unsafe fn as_ref<T>(&self) -> &T {
		assert!(Self::can_hold::<T>());

		// Safety: provided by caller
		self.transmute_ref_via_ptr(|ptr| ptr as *const T)
	}

	pub unsafe fn as_mut<T>(&mut self) -> &mut T {
		assert!(Self::can_hold::<T>());

		// Safety: provided by caller
		self.transmute_mut_via_ptr(|ptr| ptr as *mut T)
	}

	pub unsafe fn into_inner<T>(self) -> T {
		self.as_ptr::<T>().read()
	}

	pub unsafe fn drop<T>(mut self) {
		self.drop_in_place::<T>();
	}

	pub unsafe fn drop_in_place<T>(&mut self) {
		let ptr = self.as_ptr_mut::<T>();

		ptr.drop_in_place();
	}
}

// === TransMap === //

#[repr(transparent)]
pub struct TransMap<K, VHost, V, S> {
	_ty: PhantomData<V>,
	map: HashMap<K, InlineStore<VHost>, S>,
}

unsafe impl<K: Send, VHost, V: Send, S: Send> Send for TransMap<K, VHost, V, S> {}

unsafe impl<K: Sync, VHost, V: Sync, S: Sync> Sync for TransMap<K, VHost, V, S> {}

impl<K, VHost, V, S> fmt::Debug for TransMap<K, VHost, V, S>
where
	K: fmt::Debug,
	V: fmt::Debug,
{
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_map().entries(self.iter()).finish()
	}
}

impl<K, VHost, V, S: Default> Default for TransMap<K, VHost, V, S> {
	fn default() -> Self {
		assert!(InlineStore::<VHost>::can_hold::<V>());

		Self {
			_ty: Default::default(),
			map: Default::default(),
		}
	}
}

impl<K, VHost, V, S> Clone for TransMap<K, VHost, V, S>
where
	K: Clone + Hash + Eq,
	V: Clone,
	S: Clone + BuildHasher,
{
	fn clone(&self) -> Self {
		let mut map = HashMap::with_capacity_and_hasher(self.capacity(), self.map.hasher().clone());

		for (k, v) in self.iter() {
			map.insert(k.clone(), InlineStore::new(v));
		}

		Self {
			_ty: PhantomData,
			map,
		}
	}
}

impl<K, VHost, V, S> TransMap<K, VHost, V, S> {
	// pub fn hasher(&self) -> &S {
	// 	self.map.hasher()
	// }

	pub fn capacity(&self) -> usize {
		self.map.capacity()
	}

	// pub fn len(&self) -> usize {
	// 	self.map.len()
	// }

	pub fn iter(&self) -> impl ExactSizeIterator<Item = (&K, &V)> {
		self.map.iter().map(|(k, v)| (k, unsafe { v.as_ref() }))
	}

	// pub fn iter_mut(&mut self) -> impl ExactSizeIterator<Item = (&K, &mut V)> {
	// 	self.map.iter_mut().map(|(k, v)| (k, unsafe { v.as_mut() }))
	// }

	pub fn clear(&mut self) {
		for (_, value) in self.map.drain() {
			unsafe { value.drop::<V>() }
		}
	}
}

impl<K, VHost, V, S> TransMap<K, VHost, V, S>
where
	K: Hash + Eq,
	S: BuildHasher,
{
	// pub fn insert(&mut self, key: K, value: V) -> Option<V> {
	// 	self.map
	// 		.insert(key, InlineStore::new(value))
	// 		.map(|value| unsafe { value.into_inner() })
	// }

	pub fn get_mut_or_create<F>(&mut self, key: K, factory: F) -> &mut V
	where
		F: FnOnce() -> V,
	{
		let value = self
			.map
			.entry(key)
			.or_insert_with(|| InlineStore::new(factory()));

		unsafe { value.as_mut() }
	}

	pub fn remove<Q>(&mut self, key: &Q) -> Option<V>
	where
		Q: ?Sized,
		K: Borrow<Q>,
		Q: Hash + Eq,
	{
		self.map
			.remove(key)
			.map(|value| unsafe { value.into_inner() })
	}

	pub fn get<Q>(&self, key: &Q) -> Option<&V>
	where
		Q: ?Sized,
		K: Borrow<Q>,
		Q: Hash + Eq,
	{
		self.map.get(key).map(|value| unsafe { value.as_ref() })
	}

	pub fn get_mut<Q>(&mut self, key: &Q) -> Option<&mut V>
	where
		Q: ?Sized,
		K: Borrow<Q>,
		Q: Hash + Eq,
	{
		self.map.get_mut(key).map(|value| unsafe { value.as_mut() })
	}
}

impl<K, VHost, V, S> Drop for TransMap<K, VHost, V, S> {
	fn drop(&mut self) {
		self.clear();
	}
}

// === TransVec === //

#[repr(C)]
pub struct TransVec<T> {
	ptr: *mut T,
	len: usize,
	cap: usize,
}

unsafe impl<T: Send> Send for TransVec<T> {}

unsafe impl<T: Sync> Sync for TransVec<T> {}

impl<T: fmt::Debug> fmt::Debug for TransVec<T> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_list().entries(self.as_slice()).finish()
	}
}

impl<T: Clone> Clone for TransVec<T> {
	fn clone(&self) -> Self {
		Self::from_vec(Vec::from_iter(self.as_slice().iter().cloned()))
	}
}

impl<T> TransVec<T> {
	pub fn new() -> Self {
		Self::from_vec(Vec::new())
	}

	pub fn from_vec(mut vec: Vec<T>) -> Self {
		// FIXME: Replace with `into_raw_parts` once stabilized.
		let ptr = vec.as_mut_ptr();
		let len = vec.len();
		let cap = vec.capacity();
		mem::forget(vec);

		Self { ptr, len, cap }
	}

	unsafe fn as_vec(&mut self) -> Vec<T> {
		Vec::from_raw_parts(self.ptr, self.len, self.cap)
	}

	pub fn as_slice(&self) -> &[T] {
		unsafe { slice::from_raw_parts(self.ptr, self.len) }
	}

	pub fn as_mut_slice(&mut self) -> &mut [T] {
		unsafe { slice::from_raw_parts_mut(self.ptr, self.len) }
	}

	pub fn mutate<F, R>(&mut self, f: F) -> R
	where
		F: FnOnce(&mut Vec<T>) -> R,
	{
		struct Guard<'a, T> {
			target: &'a mut TransVec<T>,
			vec: ManuallyDrop<Vec<T>>,
		}

		impl<T> Drop for Guard<'_, T> {
			fn drop(&mut self) {
				// FIXME: Replace with `into_raw_parts` once stabilized.
				self.target.ptr = self.vec.as_mut_ptr();
				self.target.len = self.vec.len();
				self.target.cap = self.vec.capacity();
			}
		}

		let vec = unsafe { self.as_vec() };
		let mut guard = Guard {
			target: self,
			vec: ManuallyDrop::new(vec),
		};
		f(&mut guard.vec)
	}
}

impl<T> Drop for TransVec<T> {
	fn drop(&mut self) {
		drop(unsafe { self.as_vec() });
	}
}
*/
