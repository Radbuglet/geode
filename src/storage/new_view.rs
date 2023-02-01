use derive_where::derive_where;

// === Mapper === //

pub type FnPtrMapper<A, B> = (fn(&A) -> &B, fn(&mut A) -> &mut B);

pub trait RefMapper<I: ?Sized> {
	type Out: ?Sized;

	fn map_ref<'r>(&self, v: &'r I) -> &'r Self::Out
	where
		Self: 'r;
}

impl<I, O, F> RefMapper<I> for F
where
	I: ?Sized,
	O: ?Sized,
	F: Fn(&I) -> &O,
{
	type Out = O;

	fn map_ref<'r>(&self, v: &'r I) -> &'r Self::Out
	where
		Self: 'r,
	{
		(self)(v)
	}
}

pub trait MutMapper<I: ?Sized>: RefMapper<I> {
	fn map_mut<'r>(&self, v: &'r mut I) -> &'r mut Self::Out
	where
		Self: 'r;
}

impl<I, O, F1, F2> RefMapper<I> for (F1, F2)
where
	I: ?Sized,
	O: ?Sized,
	F1: Fn(&I) -> &O,
{
	type Out = O;

	fn map_ref<'r>(&self, v: &'r I) -> &'r Self::Out
	where
		Self: 'r,
	{
		(self.0)(v)
	}
}

impl<I, O, F1, F2> MutMapper<I> for (F1, F2)
where
	I: ?Sized,
	O: ?Sized,
	F1: Fn(&I) -> &O,
	F2: Fn(&mut I) -> &mut O,
{
	fn map_mut<'r>(&self, v: &'r mut I) -> &'r mut Self::Out
	where
		Self: 'r,
	{
		(self.1)(v)
	}
}

#[derive(Debug, Copy, Clone, Default)]
pub struct IdentityMapping;

impl<I: ?Sized> RefMapper<I> for IdentityMapping {
	type Out = I;

	fn map_ref<'r>(&self, v: &'r I) -> &'r Self::Out
	where
		Self: 'r,
	{
		v
	}
}

impl<I: ?Sized> MutMapper<I> for IdentityMapping {
	fn map_mut<'r>(&self, v: &'r mut I) -> &'r mut Self::Out
	where
		Self: 'r,
	{
		v
	}
}

#[derive(Debug)]
#[derive_where(Copy, Clone)]
pub struct MapperRefToMapper<'r, T: ?Sized>(pub &'r T);

impl<'a, I: ?Sized, T: ?Sized + RefMapper<I>> RefMapper<I> for MapperRefToMapper<'a, T> {
	type Out = T::Out;

	fn map_ref<'r>(&self, v: &'r I) -> &'r Self::Out
	where
		Self: 'r,
	{
		self.0.map_ref(v)
	}
}

impl<'a, I: ?Sized, T: ?Sized + MutMapper<I>> MutMapper<I> for MapperRefToMapper<'a, T> {
	fn map_mut<'r>(&self, v: &'r mut I) -> &'r mut Self::Out
	where
		Self: 'r,
	{
		self.0.map_mut(v)
	}
}

#[derive(Debug, Copy, Clone)]
pub struct CompositeMapper<A, B>(pub A, pub B);

impl<I: ?Sized, A: RefMapper<I>, B: RefMapper<A::Out>> RefMapper<I> for CompositeMapper<A, B> {
	type Out = B::Out;

	fn map_ref<'r>(&self, v: &'r I) -> &'r Self::Out
	where
		Self: 'r,
	{
		self.1.map_ref(self.0.map_ref(v))
	}
}

impl<I: ?Sized, A: MutMapper<I>, B: MutMapper<A::Out>> MutMapper<I> for CompositeMapper<A, B> {
	fn map_mut<'r>(&self, v: &'r mut I) -> &'r mut Self::Out
	where
		Self: 'r,
	{
		self.1.map_mut(self.0.map_mut(v))
	}
}

// === Mappable === //

pub trait Mappable {
	type Backing: ?Sized;
	type Mapper: ?Sized;

	fn as_parts(me: &Self) -> (&Self::Backing, &Self::Mapper);

	fn map<M>(
		&self,
		mapper_2: M,
	) -> MappedRef<'_, Self::Backing, CompositeMapper<MapperRefToMapper<'_, Self::Mapper>, M>> {
		let (backing, mapper_1) = Self::as_parts(self);

		MappedRef {
			backing,
			mapper: CompositeMapper(MapperRefToMapper(mapper_1), mapper_2),
		}
	}
}

pub trait MappableMut: Mappable {
	fn as_parts_mut(me: &mut Self) -> (&mut Self::Backing, &Self::Mapper);

	fn map_mut<M>(
		&mut self,
		mapper_2: M,
	) -> MappedMut<'_, Self::Backing, CompositeMapper<MapperRefToMapper<'_, Self::Mapper>, M>> {
		let (backing, mapper_1) = Self::as_parts_mut(self);

		MappedMut {
			backing,
			mapper: CompositeMapper(MapperRefToMapper(mapper_1), mapper_2),
		}
	}
}

impl<'a, T: ?Sized + Mappable> Mappable for &'a T {
	type Backing = T::Backing;
	type Mapper = T::Mapper;

	fn as_parts(me: &Self) -> (&Self::Backing, &Self::Mapper) {
		Mappable::as_parts(*me)
	}
}

impl<'a, T: ?Sized + Mappable> Mappable for &'a mut T {
	type Backing = T::Backing;
	type Mapper = T::Mapper;

	fn as_parts(me: &Self) -> (&Self::Backing, &Self::Mapper) {
		Mappable::as_parts(*me)
	}
}

impl<'a, T: ?Sized + MappableMut> MappableMut for &'a mut T {
	fn as_parts_mut(me: &mut Self) -> (&mut Self::Backing, &Self::Mapper) {
		MappableMut::as_parts_mut(*me)
	}
}

#[derive(Debug, Copy, Clone)]
pub struct MappedRef<'a, B: ?Sized, M: ?Sized> {
	pub backing: &'a B,
	pub mapper: M,
}

impl<'a, B: ?Sized, M: ?Sized> Mappable for MappedRef<'a, B, M> {
	type Backing = B;
	type Mapper = M;

	fn as_parts(me: &Self) -> (&Self::Backing, &Self::Mapper) {
		(me.backing, &me.mapper)
	}
}

#[derive(Debug)]
pub struct MappedMut<'a, B: ?Sized, M: ?Sized> {
	pub backing: &'a mut B,
	pub mapper: M,
}

impl<'a, B: ?Sized, M: ?Sized> Mappable for MappedMut<'a, B, M> {
	type Backing = B;
	type Mapper = M;

	fn as_parts(me: &Self) -> (&Self::Backing, &Self::Mapper) {
		(me.backing, &me.mapper)
	}
}

impl<'a, B: ?Sized, M: ?Sized> MappableMut for MappedMut<'a, B, M> {
	fn as_parts_mut(me: &mut Self) -> (&mut Self::Backing, &Self::Mapper) {
		(me.backing, &me.mapper)
	}
}
