use std::{
	any,
	cell::{Cell, Ref, RefCell, RefMut},
	collections::HashMap,
	fmt,
	marker::PhantomData,
	mem,
};

use fnv::FnvBuildHasher;

use crate::util::{inline::MaybeBoxedCopy, macros::impl_tuples, type_id::NamedTypeId};

// === Core === //

pub struct Provider<'r> {
	_ty: PhantomData<&'r dyn any::Any>,
	parent: Option<&'r Provider<'r>>,
	values: HashMap<NamedTypeId, ProviderEntry, FnvBuildHasher>,
}

struct ProviderEntry {
	ptr: MaybeBoxedCopy<(usize, usize)>,
	sentinel: RefCell<()>,
	readonly: Cell<bool>,
}

impl ProviderEntry {
	fn new_mut<T: ?Sized>(ptr: *mut T) -> Self {
		ProviderEntry {
			ptr: MaybeBoxedCopy::new(ptr),
			sentinel: RefCell::new(()),
			readonly: Cell::new(false),
		}
	}

	fn new_ref<T: ?Sized>(ptr: *const T) -> Self {
		let entry = ProviderEntry {
			ptr: MaybeBoxedCopy::new(ptr),
			sentinel: RefCell::new(()),
			readonly: Cell::new(false),
		};
		entry.make_readonly();
		entry
	}

	fn make_readonly(&self) {
		debug_assert!(!self.readonly.get());

		mem::forget(self.sentinel.borrow());
		self.readonly.set(true);
	}
}

impl<'r> fmt::Debug for Provider<'r> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("Provider")
			.field("parent", &self.parent)
			.field("keys", &self.values.keys().copied().collect::<Vec<_>>())
			.finish_non_exhaustive()
	}
}

impl Default for Provider<'_> {
	fn default() -> Self {
		Self::new()
	}
}

impl<'r> Provider<'r> {
	pub fn new() -> Self {
		Self {
			_ty: PhantomData,
			parent: None,
			values: HashMap::default(),
		}
	}

	pub fn new_with<T: ProviderEntries<'r>>(entries: T) -> Self {
		Self::new().with(entries)
	}

	pub fn new_inherit(parent: Option<&'r Provider<'r>>) -> Self {
		Self {
			_ty: PhantomData,
			parent,
			values: HashMap::default(),
		}
	}

	pub fn new_inherit_with<T: ProviderEntries<'r>>(
		parent: Option<&'r Provider<'r>>,
		entries: T,
	) -> Self {
		Self::new_inherit(parent).with(entries)
	}

	pub fn sub_provider<'c: 'r>(&'c self) -> Provider<'c> {
		Self::new_inherit(Some(self))
	}

	pub fn sub_provider_with<'c: 'r, T: ProviderEntries<'c>>(&'c self, entries: T) -> Provider<'c> {
		Self::new_inherit_with(Some(self), entries)
	}

	pub fn parent(&self) -> Option<&'r Provider<'r>> {
		self.parent
	}

	pub fn add_ref<T: ?Sized + 'static>(&mut self, value: &'r T) {
		self.values
			.insert(NamedTypeId::of::<T>(), ProviderEntry::new_ref(value));
	}

	pub fn add_mut<T: ?Sized + 'static>(&mut self, value: &'r mut T) {
		self.values
			.insert(NamedTypeId::of::<T>(), ProviderEntry::new_mut(value));
	}

	fn try_get_entry<T: ?Sized + 'static>(&self) -> Option<&ProviderEntry> {
		let mut iter = Some(self);

		while let Some(curr) = iter {
			if let Some(entry) = curr.values.get(&NamedTypeId::of::<T>()) {
				return Some(entry);
			}
			iter = curr.parent;
		}

		None
	}

	pub fn try_get<T: ?Sized + 'static>(&self) -> Option<Ref<T>> {
		self.try_get_entry::<T>().map(|entry| {
			let guard = match entry.sentinel.try_borrow() {
				Ok(guard) => guard,
				Err(err) => self.borrow_violation::<T, _>(entry, err, false),
			};

			Ref::map(guard, |_| unsafe {
				let ptr = entry.ptr.get::<*const T>();
				&*ptr
			})
		})
	}

	pub fn get<T: ?Sized + 'static>(&self) -> Ref<T> {
		self.try_get().unwrap_or_else(|| self.comp_not_found::<T>())
	}

	pub fn try_get_mut<T: ?Sized + 'static>(&self) -> Option<RefMut<T>> {
		self.try_get_entry::<T>().map(|entry| {
			let guard = match entry.sentinel.try_borrow_mut() {
				Ok(guard) => guard,
				Err(err) => self.borrow_violation::<T, _>(entry, err, true),
			};

			RefMut::map(guard, |_| unsafe {
				let ptr = entry.ptr.get::<*mut T>();
				&mut *ptr
			})
		})
	}

	pub fn get_mut<T: ?Sized + 'static>(&self) -> RefMut<T> {
		self.try_get_mut()
			.unwrap_or_else(|| self.comp_not_found::<T>())
	}

	pub fn try_get_frozen<T: ?Sized + 'static>(&self) -> Option<&T> {
		self.try_get_entry::<T>().map(|entry| {
			if !entry.readonly.get() {
				entry.make_readonly();
			}

			unsafe { &*entry.ptr.get::<*mut T>() }
		})
	}

	pub fn get_frozen<T: ?Sized + 'static>(&self) -> &T {
		self.try_get_frozen()
			.unwrap_or_else(|| self.comp_not_found::<T>())
	}

	fn comp_not_found<T: ?Sized + 'static>(&self) -> ! {
		panic!(
			"Could not find component of type {:?} in provider {:?}",
			NamedTypeId::of::<T>(),
			self,
		);
	}

	fn borrow_violation<T: ?Sized + 'static, E: std::error::Error>(
		&self,
		entry: &ProviderEntry,
		err: E,
		mutably: bool,
	) -> ! {
		panic!(
			"Failed to {} acquire{} component of type {:?} in provider {:?}: {}",
			if mutably { "mutably" } else { "immutably" },
			if entry.readonly.get() {
				" readonly"
			} else {
				""
			},
			NamedTypeId::of::<T>(),
			self,
			err
		);
	}
}

pub trait SpawnSubProvider {
	fn sub_provider<'c>(&'c self) -> Provider<'c>;

	fn sub_provider_with<'c, T: ProviderEntries<'c>>(&'c self, entries: T) -> Provider<'c>;
}

impl<'a> SpawnSubProvider for Provider<'a> {
	fn sub_provider<'c>(&'c self) -> Provider<'c> {
		// Name resolution prioritizes inherent method of the same name.
		self.sub_provider()
	}

	fn sub_provider_with<'c, T: ProviderEntries<'c>>(&'c self, entries: T) -> Provider<'c> {
		// Name resolution prioritizes inherent method of the same name.
		self.sub_provider_with(entries)
	}
}

// === Insertion helpers === //

impl<'r> Provider<'r> {
	pub fn with<T: ProviderEntries<'r>>(mut self, item: T) -> Self {
		item.add_to_provider(&mut self);
		self
	}
}

pub trait ProviderEntries<'a> {
	fn add_to_provider(self, provider: &mut Provider<'a>);
	fn add_to_provider_ref(&'a mut self, provider: &mut Provider<'a>);
}

impl<'a: 'b, 'b, T: ?Sized + 'static> ProviderEntries<'b> for &'a T {
	fn add_to_provider(self, provider: &mut Provider<'b>) {
		provider.add_ref(self);
	}

	fn add_to_provider_ref(&'b mut self, provider: &mut Provider<'b>) {
		provider.add_ref(*self);
	}
}

impl<'a: 'b, 'b, T: ?Sized + 'static> ProviderEntries<'b> for &'a mut T {
	fn add_to_provider(self, provider: &mut Provider<'b>) {
		provider.add_mut(self);
	}

	fn add_to_provider_ref(&'b mut self, provider: &mut Provider<'b>) {
		provider.add_mut(*self);
	}
}

macro_rules! impl_provider_entries {
	($($para:ident:$field:tt),*) => {
		impl<'a, $($para: 'a + ProviderEntries<'a>),*> ProviderEntries<'a> for ($($para,)*) {
			#[allow(unused)]
			fn add_to_provider(self, provider: &mut Provider<'a>) {
				$(self.$field.add_to_provider(&mut *provider);)*
			}

			#[allow(unused)]
			fn add_to_provider_ref(&'a mut self, provider: &mut Provider<'a>) {
				$(self.$field.add_to_provider_ref(&mut *provider);)*
			}
		}
	};
}

impl_tuples!(impl_provider_entries);

// === `unpack!` traits === //

pub trait UnpackTarget<'guard: 'borrow, 'borrow, P: ?Sized> {
	type Guard;
	type Reference;

	fn acquire_guard(src: &'guard P) -> Self::Guard;
	fn acquire_ref(guard: &'borrow mut Self::Guard) -> Self::Reference;

	#[doc(hidden)]
	fn acquire_ref_infer_src(_dummy: &P, guard: &'borrow mut Self::Guard) -> Self::Reference {
		Self::acquire_ref(guard)
	}
}

impl<'provider, 'guard: 'borrow, 'borrow, T: ?Sized + 'static>
	UnpackTarget<'guard, 'borrow, Provider<'provider>> for &'borrow T
{
	type Guard = Ref<'guard, T>;
	type Reference = Self;

	fn acquire_guard(src: &'guard Provider) -> Self::Guard {
		src.get()
	}

	fn acquire_ref(guard: &'borrow mut Self::Guard) -> Self::Reference {
		&*guard
	}
}

impl<'provider, 'guard: 'borrow, 'borrow, T: ?Sized + 'static>
	UnpackTarget<'guard, 'borrow, Provider<'provider>> for &'borrow mut T
{
	type Guard = RefMut<'guard, T>;
	type Reference = Self;

	fn acquire_guard(src: &'guard Provider) -> Self::Guard {
		src.get_mut()
	}

	fn acquire_ref(guard: &'borrow mut Self::Guard) -> Self::Reference {
		&mut *guard
	}
}

// === `unpack!` macro === //

#[doc(hidden)]
#[macro_export]
macro_rules! unpack_internal_ty_method {
	($method:ident, @arch $ty:ty) => {
		<$crate::universe::injection::ResArch<$ty> as $crate::context::UnpackTarget<_>>::$method
	};
	($method:ident, @res $ty:ty) => {
		<$crate::universe::injection::Res<&$ty> as $crate::context::UnpackTarget<_>>::$method
	};
	($method:ident, @mut $ty:ty) => {
		<$crate::universe::injection::ResRw<&mut $ty> as $crate::context::UnpackTarget<_>>::$method
	};
	($method:ident, @ref $ty:ty) => {
		<$crate::universe::injection::ResRw<&$ty> as $crate::context::UnpackTarget<_>>::$method
	};
	($method:ident, $ty:ty) => {
		<$ty as $crate::context::UnpackTarget<_>>::$method
	};
}

#[macro_export]
macro_rules! unpack {
	// Tuples
	($target:expr => (
		$($(@$anno:ident)? $comp:ty),*$(,)?
	)) => {{
		let target = $target;

		($(
			&mut $crate::unpack_internal_ty_method!(acquire_guard, $(@$anno)? $comp)(target),
		)*)
	}};

	// Statements
	($target:expr => {
		$($name:ident: $(@$anno:ident)? $comp:ty),*$(,)?
	}) => {
		let ($($name,)*) = $crate::unpack!($target => ($($(@$anno)? $comp),*));
	};

	// Combined
	($target:expr => $full_cx:ident: {
		$($stmt_name:ident: $(@$stmt_anno:ident)? $stmt_comp:ty),*
		$(
			,
			$( ...$rest_cx:ident: ($($(@$tup_anno:ident)? $tup_comp:ty),*$(,)?) $(,)? )?
		)?
	}) => {
		let target = $target;
		let mut $full_cx = (
			$(&mut $crate::unpack_internal_ty_method!(acquire_guard, $(@$stmt_anno)? $stmt_comp)(target),)*
			$($((
				$(&mut $crate::unpack_internal_ty_method!(acquire_guard, $(@$tup_anno)? $tup_comp)(target),)*
			))?)?
		);
		let ($($stmt_name,)* $($($rest_cx,)?)?) = &mut $full_cx;
		$($(let mut $rest_cx = $crate::Context::reborrow($rest_cx);)?)?
		let ($($stmt_name,)*) = ($(&mut **$stmt_name,)*);
	};
}

#[macro_export]
macro_rules! provider_from_tuple {
	($expr:expr) => {
		$crate::Provider::new_with($crate::Context::reborrow(&mut $expr))
	};
	($parent:expr, $expr:expr) => {
		$crate::context::SpawnSubProvider::sub_provider_with(
			$parent,
			$crate::Context::reborrow(&mut $expr),
		)
	};
}

pub use {provider_from_tuple, unpack};

// === Tuple context passing === //

pub use compost::{decompose, Context};
