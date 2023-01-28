pub trait PointeeCastExt {
	type Pointee: ?Sized;

	unsafe fn prolong<'r>(&self) -> &'r Self::Pointee;

	unsafe fn prolong_mut<'r>(&mut self) -> &'r mut Self::Pointee;
}

impl<P: ?Sized> PointeeCastExt for P {
	type Pointee = P;

	unsafe fn prolong<'r>(&self) -> &'r Self::Pointee {
		&*(self as *const Self::Pointee)
	}

	unsafe fn prolong_mut<'r>(&mut self) -> &'r mut Self::Pointee {
		&mut *(self as *mut Self::Pointee)
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
