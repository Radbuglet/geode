pub trait VecExt {
    type Elem;

    fn ensure_length_with<F>(&mut self, min_len: usize, f: F)
    where
        F: FnMut() -> Self::Elem;

    fn ensure_slot_with<F>(&mut self, index: usize, f: F) -> &mut Self::Elem
    where
        F: FnMut() -> Self::Elem;
}

impl<T> VecExt for Vec<T> {
    type Elem = T;

    fn ensure_length_with<F>(&mut self, min_len: usize, f: F)
    where
        F: FnMut() -> Self::Elem,
    {
        if self.len() < min_len {
            self.resize_with(min_len, f);
        }
    }

    fn ensure_slot_with<F>(&mut self, index: usize, f: F) -> &mut Self::Elem
    where
        F: FnMut() -> Self::Elem,
    {
        self.ensure_length_with(index + 1, f);
        &mut self[index]
    }
}
