use crate::{Entity, Storage, Universe};

pub trait Bundle: Sized {
	type Context<'a>;

	fn attach(self, cx: Self::Context<'_>, target: Entity);

	fn detach(cx: Self::Context<'_>, target: Entity) -> Self;

	fn attach_auto_cx(self, cx: &Universe, target: Entity);

	fn detach_auto_cx(cx: &Universe, target: Entity) -> Self;
}

#[derive(Debug, Copy, Clone, Default)]
pub struct SingletonBundle<T>(pub T);

impl<T: 'static + Send + Sync> Bundle for SingletonBundle<T> {
	type Context<'a> = &'a mut Storage<T>;

	fn attach(self, storage: Self::Context<'_>, target: Entity) {
		storage.add(target, self.0);
	}

	fn detach(storage: Self::Context<'_>, target: Entity) -> Self {
		Self(storage.try_remove(target).unwrap())
	}

	fn attach_auto_cx(self, cx: &Universe, target: Entity) {
		cx.storage_mut::<T>().add(target, self.0);
	}

	fn detach_auto_cx(cx: &Universe, target: Entity) -> Self {
		Self(cx.storage_mut::<T>().try_remove(target).unwrap())
	}
}

#[macro_export]
macro_rules! bundle {
	($(
		$(#[$attr_meta:meta])*
		$vis:vis struct $name:ident {
			$(
				$(#[$field_meta:meta])*
				$field_vis:vis $field:ident: $ty:ty
			),*
			$(,)?
		}
	)*) => {$(
		$(#[$attr_meta])*
		$vis struct $name {
			$(
				$(#[$field_meta])*
				$field_vis $field: $ty
			),*
		}

		impl $crate::Bundle for $name {
			type Context<'a> = ($(&'a mut $crate::Storage<$ty>,)*);

			#[allow(unused)]
			fn attach(self, ($($field,)*): Self::Context<'_>, target: $crate::Entity) {
				$( $field.add(target, self.$field); )*
			}

			#[allow(unused)]
			fn detach(($($field,)*): Self::Context<'_>, target: $crate::Entity) -> Self {
				$( let $field = $field.try_remove(target).unwrap(); )*

				Self { $($field),* }
			}

			#[allow(unused)]
			fn attach_auto_cx(self, cx: &$crate::Universe, target: $crate::Entity) {
				$( cx.storage_mut::<$ty>().add(target, self.$field); )*
			}

			#[allow(unused)]
			fn detach_auto_cx(cx: &$crate::Universe, target: $crate::Entity) -> Self {
				$( let $field = cx.storage_mut::<$ty>().try_remove(target).unwrap(); )*

				Self { $($field),* }
			}
		}
	)*};
}

pub use bundle;
