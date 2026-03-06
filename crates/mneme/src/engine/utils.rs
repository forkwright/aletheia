// Originally derived from CozoDB v0.7.6 (MPL-2.0).
// Copyright 2022, The Cozo Project Authors — see NOTICE for details.

#[inline(always)]
pub(crate) fn swap_option_result<T, E>(d: Result<Option<T>, E>) -> Option<Result<T, E>> {
    match d {
        Ok(Some(s)) => Some(Ok(s)),
        Ok(None) => None,
        Err(e) => Some(Err(e)),
    }
}

#[derive(Default)]
pub(crate) struct TempCollector<T> {
    pub(crate) inner: Vec<T>,
}

impl<T> TempCollector<T> {
    pub(crate) fn push(&mut self, val: T) {
        self.inner.push(val);
    }
    pub(crate) fn into_iter(self) -> impl Iterator<Item = T> {
        self.inner.into_iter()
    }
}
