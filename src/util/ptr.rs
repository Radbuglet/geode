pub trait PointeeCastExt {
	type Pointee: ?Sized;

	unsafe fn prolong<'r>(&self) -> &'r Self::Pointee;

	unsafe fn prolong_mut<'r>(&mut self) -> &'r mut Self::Pointee;

	unsafe fn transmute_ref_via_ptr<F, R>(&self, f: F) -> &R
	where
		R: ?Sized,
		F: FnOnce(*const Self::Pointee) -> *const R;

	unsafe fn transmute_mut_via_ptr<F, R>(&mut self, f: F) -> &mut R
	where
		R: ?Sized,
		F: FnOnce(*mut Self::Pointee) -> *mut R;
}

impl<P: ?Sized> PointeeCastExt for P {
	type Pointee = P;

	unsafe fn prolong<'r>(&self) -> &'r Self::Pointee {
		&*(self as *const Self::Pointee)
	}

	unsafe fn prolong_mut<'r>(&mut self) -> &'r mut Self::Pointee {
		&mut *(self as *mut Self::Pointee)
	}

	unsafe fn transmute_ref_via_ptr<F, R>(&self, f: F) -> &R
	where
		R: ?Sized,
		F: FnOnce(*const Self::Pointee) -> *const R,
	{
		&*f(self)
	}

	unsafe fn transmute_mut_via_ptr<F, R>(&mut self, f: F) -> &mut R
	where
		R: ?Sized,
		F: FnOnce(*mut Self::Pointee) -> *mut R,
	{
		&mut *f(self)
	}
}

// FIXME: These methods are likely to cause U.B.
//  See: https://blog.nilstrieb.dev/posts/box-is-a-unique-type/
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
