use std::mem::{self, ManuallyDrop};

// === Transmute === //

pub const unsafe fn entirely_unchecked_transmute<A, B>(a: A) -> B {
	union Punny<A, B> {
		a: ManuallyDrop<A>,
		b: ManuallyDrop<B>,
	}

	let punned = Punny {
		a: ManuallyDrop::new(a),
	};

	ManuallyDrop::into_inner(punned.b)
}

pub const unsafe fn sizealign_checked_transmute<A, B>(a: A) -> B {
	assert!(mem::size_of::<A>() == mem::size_of::<B>());
	assert!(mem::align_of::<A>() >= mem::align_of::<B>());

	entirely_unchecked_transmute(a)
}

// === Allocation === //

pub fn leak_on_heap<'a, T>(val: T) -> &'a mut T {
	Box::leak(Box::new(val))
}

// === Pointer Casts === //

pub trait PointeeCastExt {
	type Pointee: ?Sized;

	fn as_byte_ptr(&self) -> *const u8;

	unsafe fn prolong<'r>(&self) -> &'r Self::Pointee;

	unsafe fn prolong_mut<'r>(&mut self) -> &'r mut Self::Pointee;

	unsafe fn cast_ref_via_ptr<F, R>(&self, f: F) -> &R
	where
		R: ?Sized,
		F: FnOnce(*const Self::Pointee) -> *const R;

	unsafe fn cast_mut_via_ptr<F, R>(&mut self, f: F) -> &mut R
	where
		R: ?Sized,
		F: FnOnce(*mut Self::Pointee) -> *mut R;

	unsafe fn try_cast_ref_via_ptr<F, R, E>(&self, f: F) -> Result<&R, E>
	where
		R: ?Sized,
		F: FnOnce(*const Self::Pointee) -> Result<*const R, E>;

	unsafe fn try_cast_mut_via_ptr<F, R, E>(&mut self, f: F) -> Result<&mut R, E>
	where
		R: ?Sized,
		F: FnOnce(*mut Self::Pointee) -> Result<*mut R, E>;

	unsafe fn transmute_pointee_ref<T: ?Sized>(&self) -> &T;

	unsafe fn transmute_pointee_mut<T: ?Sized>(&mut self) -> &mut T;
}

impl<P: ?Sized> PointeeCastExt for P {
	type Pointee = P;

	fn as_byte_ptr(&self) -> *const u8 {
		(self as *const Self).cast::<u8>()
	}

	unsafe fn prolong<'r>(&self) -> &'r Self::Pointee {
		&*(self as *const Self::Pointee)
	}

	unsafe fn prolong_mut<'r>(&mut self) -> &'r mut Self::Pointee {
		&mut *(self as *mut Self::Pointee)
	}

	unsafe fn cast_ref_via_ptr<F, R>(&self, f: F) -> &R
	where
		R: ?Sized,
		F: FnOnce(*const Self::Pointee) -> *const R,
	{
		&*f(self)
	}

	unsafe fn cast_mut_via_ptr<F, R>(&mut self, f: F) -> &mut R
	where
		R: ?Sized,
		F: FnOnce(*mut Self::Pointee) -> *mut R,
	{
		&mut *f(self)
	}

	unsafe fn try_cast_ref_via_ptr<F, R, E>(&self, f: F) -> Result<&R, E>
	where
		R: ?Sized,
		F: FnOnce(*const Self::Pointee) -> Result<*const R, E>,
	{
		Ok(&*f(self)?)
	}

	unsafe fn try_cast_mut_via_ptr<F, R, E>(&mut self, f: F) -> Result<&mut R, E>
	where
		R: ?Sized,
		F: FnOnce(*mut Self::Pointee) -> Result<*mut R, E>,
	{
		Ok(&mut *f(self)?)
	}

	unsafe fn transmute_pointee_ref<T: ?Sized>(&self) -> &T {
		sizealign_checked_transmute(self)
	}

	unsafe fn transmute_pointee_mut<T: ?Sized>(&mut self) -> &mut T {
		sizealign_checked_transmute(self)
	}
}

pub trait HeapPointerExt {
	type Pointee: ?Sized;

	unsafe fn prolong_heap_ref<'a>(&self) -> &'a Self::Pointee;
}

impl<T: ?Sized> HeapPointerExt for Box<T> {
	type Pointee = T;

	unsafe fn prolong_heap_ref<'a>(&self) -> &'a Self::Pointee {
		(**self).prolong()
	}
}

pub fn addr_of_ptr<T: ?Sized>(p: *const T) -> usize {
	p.cast::<()>() as usize
}
