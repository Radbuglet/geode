use std::{
	any,
	cell::{Ref, RefCell, RefMut},
	collections::HashMap,
	fmt,
	marker::PhantomData,
	mem,
};

use fnv::FnvBuildHasher;

use crate::{
	util::{inline::MaybeBoxedCopy, macros::impl_tuples, type_id::NamedTypeId},
	Universe,
};

// === Core === //

pub struct Provider<'r> {
	_ty: PhantomData<&'r dyn any::Any>,
	universe: &'r Universe,
	parent: Option<&'r Provider<'r>>,
	values: HashMap<NamedTypeId, (MaybeBoxedCopy<(usize, usize)>, RefCell<()>), FnvBuildHasher>,
}

impl<'r> fmt::Debug for Provider<'r> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("Provider")
			.field("parent", &self.parent)
			.field("keys", &self.values.keys().copied().collect::<Vec<_>>())
			.finish_non_exhaustive()
	}
}

impl<'r> Provider<'r> {
	pub fn new(universe: &'r Universe) -> Self {
		Self {
			_ty: PhantomData,
			universe,
			parent: None,
			values: HashMap::default(),
		}
	}

	pub fn new_with<T: ProviderEntries<'r>>(universe: &'r Universe, entries: T) -> Self {
		Self::new(universe).with(entries)
	}

	pub fn new_with_parent(parent: &'r Provider<'r>) -> Self {
		Self {
			_ty: PhantomData,
			universe: parent.universe,
			parent: Some(parent),
			values: HashMap::default(),
		}
	}

	pub fn new_with_parent_and_comps<T: ProviderEntries<'r>>(
		parent: &'r Provider<'r>,
		entries: T,
	) -> Self {
		Self::new_with_parent(parent).with(entries)
	}

	pub fn spawn_child<'c>(&'c self) -> Provider<'c> {
		Provider::new_with_parent(self)
	}

	pub fn spawn_child_with<'c, T: ProviderEntries<'c>>(&'c self, entries: T) -> Provider<'c> {
		Provider::new_with_parent(self).with(entries)
	}

	pub fn parent(&self) -> Option<&'r Provider<'r>> {
		self.parent
	}

	pub fn universe(&self) -> &Universe {
		self.universe
	}

	pub fn add_ref<T: ?Sized + 'static>(&mut self, value: &'r T) {
		let sentinel = RefCell::new(());
		mem::forget(sentinel.borrow());

		self.values.insert(
			NamedTypeId::of::<T>(),
			(MaybeBoxedCopy::new(value as *const T), sentinel),
		);
	}

	pub fn add_mut<T: ?Sized + 'static>(&mut self, value: &'r mut T) {
		self.values.insert(
			NamedTypeId::of::<T>(),
			(MaybeBoxedCopy::new(value as *const T), RefCell::new(())),
		);
	}

	fn try_get_entry<T: ?Sized + 'static>(
		&self,
	) -> Option<&(MaybeBoxedCopy<(usize, usize)>, RefCell<()>)> {
		if NamedTypeId::of::<T>() == NamedTypeId::of::<Universe>() {
			log::warn!(
				"Attempting to fetch a `Universe` component from a `Provider`. \
			     This is likely an error because `universes` are passed as a field in the `Provider` \
				 and are accessible through `Provider::universe()` and are therefore almost never passed \
				 as a component."
			);
		}

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
		self.try_get_entry::<T>().map(|(ptr, sentinel)| {
			let guard = sentinel.borrow();

			Ref::map(guard, |_| unsafe {
				let ptr = ptr.get::<*const T>();
				&*ptr
			})
		})
	}

	pub fn get<T: ?Sized + 'static>(&self) -> Ref<T> {
		self.try_get().unwrap_or_else(|| self.comp_not_found::<T>())
	}

	pub fn try_get_mut<T: ?Sized + 'static>(&self) -> Option<RefMut<T>> {
		self.try_get_entry::<T>().map(|(ptr, sentinel)| {
			let guard = sentinel.borrow_mut();

			RefMut::map(guard, |_| unsafe {
				let ptr = ptr.get::<*mut T>();
				&mut *ptr
			})
		})
	}

	pub fn get_mut<T: ?Sized + 'static>(&self) -> RefMut<T> {
		self.try_get_mut()
			.unwrap_or_else(|| self.comp_not_found::<T>())
	}

	fn comp_not_found<T: ?Sized + 'static>(&self) -> ! {
		panic!(
			"Could not find component of type {:?} in provider {:?}",
			NamedTypeId::of::<T>(),
			self,
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
pub mod macro_internal {
	use super::*;

	// === `unpack!` stuff === //

	pub use std::marker::PhantomData;

	pub trait UnpackTargetTuple<'guard: 'borrow, 'borrow, P: ?Sized, I> {
		type Output;

		fn acquire_refs(self, _dummy_provider: &P, input: &'borrow mut I) -> Self::Output;
	}

	macro_rules! impl_guard_tuples_as_refs {
		($($para:ident:$field:tt),*) => {
			impl<'guard: 'borrow, 'borrow, P: ?Sized, $($para: UnpackTarget<'guard, 'borrow, P>),*>
				UnpackTargetTuple<'guard, 'borrow, P, ($($para::Guard,)*)>
				for ($(PhantomData<$para>,)*)
			{
				type Output = ($($para::Reference,)*);

				#[allow(unused)]
				#[allow(clippy::unused_unit)]
				fn acquire_refs(self, _dummy_provider: &P, guards: &'borrow mut ($($para::Guard,)*)) -> Self::Output {
					($($para::acquire_ref(&mut guards.$field),)*)
				}
			}
		};
	}

	impl_tuples!(impl_guard_tuples_as_refs);

	// TODO: It may look like we could take these two expression macros and combine them into one
	// type macro, which would be nice, but rust-analyzer doesn't seem to support type macros correctly
	// so this is the easiest way to preserve IDE completions while implementing this feature.
	#[doc(hidden)]
	#[macro_export]
	macro_rules! unpack_internal_ty_acquire_guard {
		($src:expr, @arch $ty:ty) => {
			<$crate::universe::injection::ResArch<$ty> as $crate::context::UnpackTarget<_>>::acquire_guard(
				$src,
			)
		};
		($src:expr, @res $ty:ty) => {
			<$crate::universe::injection::Res<&$ty> as $crate::context::UnpackTarget<_>>::acquire_guard($src)
		};
		($src:expr, @mut $ty:ty) => {
			<$crate::universe::injection::ResRw<&mut $ty> as $crate::context::UnpackTarget<_>>::acquire_guard(
				$src,
			)
		};
		($src:expr, @ref $ty:ty) => {
			<$crate::universe::injection::ResRw<&$ty> as $crate::context::UnpackTarget<_>>::acquire_guard($src)
		};
		($src:expr, $ty:ty) => {
			<$ty as $crate::context::UnpackTarget<_>>::acquire_guard($src)
		};
	}

	#[doc(hidden)]
	#[macro_export]
	macro_rules! unpack_internal_ty_phantom_data {
		(@arch $ty:ty) => {
			$crate::context::macro_internal::PhantomData::<$crate::universe::injection::ResArch<$ty>>
		};
		(@res $ty:ty) => {
			$crate::context::macro_internal::PhantomData::<$crate::universe::injection::Res<&$ty>>
		};
		(@mut $ty:ty) => {
			$crate::context::macro_internal::PhantomData::<
				$crate::universe::injection::ResRw<&mut $ty>,
			>
		};
		(@ref $ty:ty) => {
			$crate::context::macro_internal::PhantomData::<$crate::universe::injection::ResRw<&$ty>>
		};
		($ty:ty) => {
			$crate::context::macro_internal::PhantomData::<$ty>
		};
	}

	// === `provider_from_tuple!` macro === //

	pub struct ProviderFromDecomposedTuple<T>(pub T);

	macro_rules! impl_provider_entries {
		($($para:ident:$field:tt),*) => {
			impl<'a, $($para: 'a + ProviderEntries<'a>),*>
				ProviderEntries<'a> for
				ProviderFromDecomposedTuple<($(&'a mut $para,)*)>
			{
				#[allow(unused)]
				fn add_to_provider(self, provider: &mut Provider<'a>) {
					$(self.0.$field.add_to_provider_ref(&mut *provider);)*
				}

				#[allow(unused)]
				fn add_to_provider_ref(&'a mut self, provider: &mut Provider<'a>) {
					$(self.0.$field.add_to_provider_ref(&mut *provider);)*
				}
			}
		};
	}

	impl_tuples!(impl_provider_entries);
}

#[macro_export]
macro_rules! unpack {
	// Guarded struct unpack with context
	($out_tup:ident & $out_tup_rest:ident = $src:expr => {
		$(
			$name_bound:ident: $(@$anno_bound:ident)? $ty_bound:ty
		),*
		$(, $(...:
			$($(@$anno_unbound:ident)? $ty_unbound:ty),*
			$(,)?
		)?)?
	}) => {
		let src = $src;
		let mut guard;
		let mut $out_tup = $crate::unpack!(src => guard & (
			$($(@$anno_bound)? $ty_bound,)*
			$($(
				$($(@$anno_unbound)? $ty_unbound,)*
			)?)?
		));

		let (($($name_bound,)*), mut $out_tup_rest) = {
			#[allow(non_camel_case_types)]
			fn identity_helper<'guard: 'borrow, 'borrow, P: ?Sized, R, $($name_bound: $crate::context::UnpackTarget<'guard, 'borrow, P>),*>(
				_dummy_provider: &P,
				_dummy_targets: ($($crate::context::macro_internal::PhantomData<$name_bound>,)*),
				v: (($(<$name_bound as $crate::context::UnpackTarget<'guard, 'borrow, P>>::Reference,)*), R),
			) -> (($(<$name_bound as $crate::context::UnpackTarget<'guard, 'borrow, P>>::Reference,)*), R) {
				v
			}

			identity_helper(
				src,
				($($crate::unpack_internal_ty_phantom_data!($(@$anno_bound)? $ty_bound),)*),
				$crate::decompose!(...$out_tup),
			)
		};
	};
	($out_tup:ident = $src:expr => {
		$(
			$name_bound:ident: $(@$anno_bound:ident)? $ty_bound:ty
		),*
		$(, $(...:
			$($(@$anno_unbound:ident)? $ty_unbound:ty),*
			$(,)?
		)?)?
	}) => {
		$crate::unpack!($out_tup & $out_tup = $src => {
			$(
				$name_bound: $(@$anno_bound)? $ty_bound
			),*
			$(
				,
				$(...:
					$($(@$anno_unbound)? $ty_unbound),*
				)?
			)?
		});
	};
	($out_tup:ident = $src:expr => (
		$($(@$anno_unbound:ident)? $ty_unbound:ty),*
		$(,)?
	)) => {
		$crate::unpack!($out_tup & _ignore = $src => {
			,...: $(
				$(@$anno_unbound)?
				$ty_unbound
			),*
		});
	};

	// Guarded tuple unpack
	($src:expr => $guard:ident & (
		$(
			$(@$anno:ident)? $ty:ty
		),*
		$(,)?
	)) => {{
		// Solidify reference
		let src = $src;

		// Acquire guards
		$guard = ($( $crate::unpack_internal_ty_acquire_guard!(src, $(@$anno)? $ty) ,)*);

		// Acquire references
		$crate::context::macro_internal::UnpackTargetTuple::acquire_refs(
			($($crate::unpack_internal_ty_phantom_data!($(@$anno)? $ty),)*),
			src,
			&mut $guard,
		)
	}};

	// Unguarded tuple unpack
	($src:expr => (
		$(
			$(@$anno:ident)? $ty:ty
		),*
		$(,)?
	)) => {{
		let src = $src;
		($( $crate::unpack_internal_ty_acquire_guard!(src, $(@$anno)? $ty) ,)*)
	}};

	// Guarded struct unpack
	($src:expr => {
		$(
			$name:ident: $(@$anno:ident)? $ty:ty
		),*
		$(,)?
	}) => {
		let mut guard;
		let ($($name,)*) = $crate::unpack!($src => guard & (
			$($(@$anno)? $ty),*
		));
	};

	// Unguarded struct unpack
	($src:expr => {
		$(
			$name:pat = $(@$anno:ident)? $ty:ty
		),*
		$(,)?
	}) => {
		let ($($name,)*) = $crate::unpack!($src => (
			$($(@$anno)? $ty),*
		));
	};
}

#[macro_export]
macro_rules! provider_from_tuple {
	($parent:expr, $expr:expr) => {
		$crate::Provider::new_with_parent_and_comps(
			$parent,
			$crate::context::macro_internal::ProviderFromDecomposedTuple(
				$crate::decompose!($expr => (&mut ...))
			)
		)
	};
	($expr:expr) => {
		$crate::Provider::new_with(
			$crate::ecs::context::macro_internal::ProviderFromDecomposedTuple(
				$crate::decompose!($expr => (&mut ...))
			)
		)
	};
}

pub use {provider_from_tuple, unpack};

// === Tuple context passing === //

pub use compost::decompose;
pub use tuples::{CombinConcat, CombinRight};
