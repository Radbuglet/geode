use std::{
	any,
	cell::{Cell, Ref, RefCell, RefMut},
	collections::HashMap,
	fmt,
	marker::PhantomData,
	mem,
	ops::Deref,
};

use fnv::FnvBuildHasher;

use crate::util::{inline::MaybeBoxedCopy, macros::impl_tuples, type_id::NamedTypeId};

// === Core === //

pub struct Provider<'r> {
	_ty: PhantomData<&'r dyn any::Any>,
	parent: Option<&'r Provider<'r>>,
	values: HashMap<NamedTypeId, ProviderEntry, FnvBuildHasher>,
	is_exclusive: bool,
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
			is_exclusive: true,
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
			is_exclusive: false,
		}
	}

	pub fn new_inherit_with<T: ProviderEntries<'r>>(
		parent: Option<&'r Provider<'r>>,
		entries: T,
	) -> Self {
		Self::new_inherit(parent).with(entries)
	}

	pub fn new_inherit_exclusive_unchecked<'r2: 'r>(parent: Option<&'r Provider<'r>>) -> Self {
		Self {
			_ty: PhantomData,
			parent,
			values: HashMap::default(),
			is_exclusive: true,
		}
	}

	pub fn new_inherit_exclusive_unchecked_with<'r2: 'r, T: ProviderEntries<'r>>(
		parent: Option<&'r Provider<'r>>,
		entries: T,
	) -> Self {
		Self::new_inherit_exclusive_unchecked(parent).with(entries)
	}

	pub fn new_inherit_exclusive<'r2: 'r>(parent: Option<&mut ExclusiveProvider<'r2>>) -> Self {
		Self::new_inherit_exclusive_unchecked(parent.map(|parent| parent.0))
	}

	pub fn new_inherit_exclusive_with<'r2: 'r, T: ProviderEntries<'r>>(
		parent: Option<&mut ExclusiveProvider<'r2>>,
		entries: T,
	) -> Self {
		Self::new_inherit_exclusive(parent).with(entries)
	}

	pub fn is_exclusive(&self) -> bool {
		self.is_exclusive
	}

	pub fn as_exclusive(&mut self) -> ExclusiveProvider<'_> {
		ExclusiveProvider::new(self)
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

// === BypassExclusivity === //

pub trait BypassExclusivity {}

pub trait UnpackTargetBypassesExclusivity {}

impl<T: ?Sized + 'static + BypassExclusivity> UnpackTargetBypassesExclusivity for &'_ T {}

impl<T: ?Sized + 'static + BypassExclusivity> UnpackTargetBypassesExclusivity for &'_ mut T {}

// === ExclusiveProvider === //

/// An `ExclusiveProvider` is a new-typed [`Provider`] that encodes the assumption of being
/// entirely unborrowed into the type-system. In essence, they're like a mutable reference to
/// a [`Provider`] with special escape-hatches to make them useful in practice.
///
/// `ExclusiveProviders` immutably dereference to [`Provider`]s meaning that, to actually
/// benefit from the guarantee of exclusivity, you must typically pass a mutable reference to
/// your `ExclusiveProvider`. Transfer of ownership could also work well, although doing so would
/// prevent you from reborrowing.
///
/// Where `ExclusiveProviders` shine is their two escape hatches:
///
/// - [`ExclusiveProvider::escape_safe`], which returns a [`BypassOnlyExclusiveProvider`] with a
///   lifetime independent of the lifetime of the borrow. This `BypassOnlyExclusiveProvider` instance
///   allows you to access components implementing the [`BypassExclusivity`] trait. In other words,
///   you can hold onto a `BypassOnlyExclusiveProvider` and borrow a select few components from it
///   while you pass the `ExclusiveProvider` instance to another method.
///
/// - [`ExclusiveProvider::escape_unchecked`], which returns a [`Provider`] with a lifetime independent
///   of the lifetime of the borrow. Once you do this, however, all bets are off! Be very careful
///   about how you use this.
///
#[derive(Debug)]
pub struct ExclusiveProvider<'r>(&'r Provider<'r>);

impl<'r> ExclusiveProvider<'r> {
	pub fn new<'r2: 'r>(provider: &'r mut Provider<'r2>) -> Self {
		assert!(provider.is_exclusive());

		Self::unchecked_new(provider)
	}

	pub fn unchecked_new(provider: &'r Provider<'r>) -> Self {
		Self(provider)
	}

	pub fn into_raw_provider(self) -> &'r Provider<'r> {
		self.0
	}

	pub fn escape_safe(&self) -> BypassOnlyExclusiveProvider<'r> {
		BypassOnlyExclusiveProvider(self.0)
	}

	pub fn escape_unchecked(&self) -> &'r Provider<'r> {
		self.0
	}

	pub fn sub_provider_exclusive(&mut self) -> Provider<'_> {
		Provider::new_inherit_exclusive(Some(self))
	}

	pub fn sub_provider_exclusive_with<'s, T: ProviderEntries<'s>>(
		&'s mut self,
		entries: T,
	) -> Provider<'s> {
		Provider::new_inherit_exclusive_with(Some(self), entries)
	}
}

impl<'r> Deref for ExclusiveProvider<'r> {
	type Target = Provider<'r>;

	fn deref(&self) -> &Self::Target {
		self.0
	}
}

#[derive(Debug, Copy, Clone)]
pub struct BypassOnlyExclusiveProvider<'r>(&'r Provider<'r>);

impl<'r> BypassOnlyExclusiveProvider<'r> {
	pub fn unchecked_new(target: &'r Provider<'r>) -> Self {
		Self(target)
	}

	pub fn unchecked_raw(&self) -> &'r Provider<'r> {
		self.0
	}
}

impl<'p, 'guard, 'borrow, T> UnpackTarget<'guard, 'borrow, BypassOnlyExclusiveProvider<'p>> for T
where
	'guard: 'borrow,
	T: UnpackTarget<'guard, 'borrow, Provider<'p>> + UnpackTargetBypassesExclusivity,
{
	type Guard = T::Guard;
	type Reference = T::Reference;

	fn acquire_guard(src: &'guard BypassOnlyExclusiveProvider<'p>) -> Self::Guard {
		T::acquire_guard(src.unchecked_raw())
	}

	fn acquire_ref(guard: &'borrow mut Self::Guard) -> Self::Reference {
		T::acquire_ref(guard)
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

pub use unpack;

// === Tuple context passing === //

pub use compost::{decompose, Context};
