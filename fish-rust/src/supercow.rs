use std::borrow::Borrow;
use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::Deref;

/// A Cow that supports the usual `Borrowed` and `Owned` variants as well as a `Shared` variant that
/// can hold an `Arc` or `Rc` of the variable.
pub enum SuperCow<'a, T: ?Sized + ToOwned, S>
where
    S: Deref<Target = T::Owned>,
{
    Owned(T::Owned),
    Borrowed(&'a T),
    Shared(S),
}

// Type bounds on type aliases are not supported so we can't do this :'(
// pub type ArcCow<'a, B: ToOwned> = SuperCow<'a, B, Arc<B::Owned>>;
pub type ArcCow<'a, B, Owned> = SuperCow<'a, B, std::sync::Arc<Owned>>;
pub type RcCow<'a, B, Owned> = SuperCow<'a, B, std::rc::Rc<Owned>>;

impl<'a, T: ?Sized + ToOwned, S> Deref for SuperCow<'a, T, S>
where
    S: Deref<Target = T::Owned>,
{
    type Target = T;

    fn deref(&self) -> &Self::Target {
        match self {
            SuperCow::Owned(owned) => owned.borrow(),
            SuperCow::Borrowed(borrow) => borrow,
            SuperCow::Shared(shared) => shared.deref().borrow(),
        }
    }
}

impl<'a, T: ?Sized + ToOwned, S> SuperCow<'a, T, S>
where
    S: Deref<Target = T::Owned>,
{
    pub fn into_owned(self) -> <T as ToOwned>::Owned {
        match self {
            SuperCow::Owned(owned) => owned,
            SuperCow::Borrowed(borrow) => borrow.to_owned(),
            SuperCow::Shared(shared) => shared.deref().borrow().to_owned(),
        }
    }
}

impl<'a, T: ?Sized + ToOwned, S> Clone for SuperCow<'a, T, S>
where
    S: Deref<Target = T::Owned> + Clone,
{
    fn clone(&self) -> Self {
        match self {
            SuperCow::Owned(owned) => SuperCow::Owned(owned.borrow().to_owned()),
            SuperCow::Borrowed(borrow) => SuperCow::Borrowed(borrow),
            SuperCow::Shared(shared) => SuperCow::Shared(shared.clone()),
        }
    }
}

impl<'a, 'b, T: ?Sized, S> PartialOrd for SuperCow<'a, T, S>
where
    T: ToOwned + PartialOrd,
    S: Deref<Target = T::Owned>,
{
    #[inline]
    fn partial_cmp(&self, other: &SuperCow<'a, T, S>) -> Option<Ordering> {
        PartialOrd::partial_cmp(&**self, &**other)
    }
}

impl<'a, 'b, T: ?Sized, S> Ord for SuperCow<'a, T, S>
where
    T: ToOwned + Ord,
    S: Deref<Target = T::Owned>,
{
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        Ord::cmp(&**self, &**other)
    }
}

impl<'a, 'b, T: ?Sized, S1, V: ?Sized + ToOwned, S2> PartialEq<SuperCow<'b, V, S2>>
    for SuperCow<'a, T, S1>
where
    T: ToOwned + PartialEq<V>,
    S1: Deref<Target = T::Owned>,
    S2: Deref<Target = V::Owned>,
{
    #[inline]
    fn eq(&self, other: &SuperCow<'b, V, S2>) -> bool {
        PartialEq::eq(&**self, &**other)
    }
}

impl<'a, 'b, T: ?Sized, S> Eq for SuperCow<'a, T, S>
where
    T: ToOwned + Eq,
    S: Deref<Target = T::Owned>,
{
}

impl<'a, T: ?Sized, S> fmt::Debug for SuperCow<'a, T, S>
where
    T: ToOwned + fmt::Debug,
    T::Owned: fmt::Debug,
    S: Deref<Target = T::Owned>,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SuperCow::Owned(o) => fmt::Debug::fmt(o, f),
            SuperCow::Borrowed(b) => fmt::Debug::fmt(b, f),
            SuperCow::Shared(s) => fmt::Debug::fmt(s.deref(), f),
        }
    }
}

impl<'a, T: ?Sized, S> fmt::Display for SuperCow<'a, T, S>
where
    T: ToOwned + fmt::Display,
    T::Owned: fmt::Display,
    S: Deref<Target = T::Owned>,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SuperCow::Owned(o) => fmt::Display::fmt(o, f),
            SuperCow::Borrowed(b) => fmt::Display::fmt(b, f),
            SuperCow::Shared(s) => fmt::Display::fmt(s.deref(), f),
        }
    }
}

impl<'a, T: ?Sized, S> Hash for SuperCow<'a, T, S>
where
    T: ToOwned + Hash,
    T::Owned: Hash,
    S: Deref<Target = T::Owned>,
{
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        Hash::hash(&**self, state)
    }
}

impl<'a, T: ?Sized, S> AsRef<T> for SuperCow<'a, T, S>
where
    T: ToOwned,
    S: Deref<Target = T::Owned>,
{
    fn as_ref(&self) -> &T {
        self
    }
}
