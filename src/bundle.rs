use crate::{Entity, Storage};

#[derive(Debug, Copy, Clone, Default)]
pub struct SingletonBundle<T>(pub T);

impl<T: 'static> Bundle for SingletonBundle<T> {
	type Context<'a> = &'a mut Storage<T>;

	fn attach(self, storage: Self::Context<'_>, target: Entity) {
		storage.add(target, self.0);
	}

	fn detach(storage: Self::Context<'_>, target: Entity) -> Self {
		Self(storage.try_remove(target).unwrap())
	}
}

pub trait Bundle: Sized {
	type Context<'a>;

	fn attach(self, cx: Self::Context<'_>, target: Entity);
	fn detach(cx: Self::Context<'_>, target: Entity) -> Self;
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
			fn attach(self, mut cx: Self::Context<'_>, target: $crate::Entity) {
				$(
					$crate::decompose!(cx => {
						storage: &mut $crate::Storage<$ty>
					});
					storage.add(target, self.$field);
				)*
			}

			#[allow(unused)]
			fn detach(mut cx: Self::Context<'_>, target: $crate::Entity) -> Self {
				$(
					$crate::decompose!(cx => {
						storage: &mut $crate::Storage<$ty>
					});
					let $field = storage.try_remove(target).unwrap();
				)*

				Self { $($field),* }
			}
		}
	)*};
}

pub use bundle;
