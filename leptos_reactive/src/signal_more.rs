use crate::{
    create_rw_signal, store_value, RwSignal, Scope, Signal, SignalDispose,
    SignalGet, SignalGetUntracked, SignalSet, SignalUpdate, SignalWith,
    SignalWithUntracked,
};
use std::{
    hash::{Hash, Hasher},
    ops::Deref,
    rc::Rc,
};

/// A leaked [RwSignal].
/// It is important that there are no ways to construct this without leaking it, including during deserialisation.
///
/// This is equivalent to a [RwSignal], but it is never disposed of unless done manually by the user
/// or the runtime itself is disposed of.
///
/// Useful to statically guarantee a global signal is not disposed of accidentally.
#[derive(PartialEq, Eq)]
pub struct LeakedRwSignal<T: 'static>(RwSignal<T>);
impl<T: 'static> LeakedRwSignal<T> {
    /// Creates a new [LeakedRwSignal], see type docs for more.
    #[inline(always)]
    #[track_caller]
    pub fn new(value: T) -> Self {
        Self(Scope::LEAK.with_owner(move || create_rw_signal(value)))
    }
}
impl<T: 'static + std::fmt::Debug> std::fmt::Debug for LeakedRwSignal<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = f.debug_struct("LeakedRwSignal");
        self.with_untracked(|v| s.field("inner", v));
        #[cfg(any(debug_assertions, feature = "ssr"))]
        s.field("defined_at", &self.0.defined_at);
        s.finish()
    }
}
impl<T: 'static> Copy for LeakedRwSignal<T> {}
impl<T: 'static> Clone for LeakedRwSignal<T> {
    #[allow(clippy::non_canonical_clone_impl)] // We don't need the T: Clone bound.
    fn clone(&self) -> Self {
        Self(self.0)
    }
}
impl<T: Hash + 'static> Hash for LeakedRwSignal<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.with(|v| v.hash(state));
    }
}
impl<T: 'static> Deref for LeakedRwSignal<T> {
    type Target = RwSignal<T>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl<T: Default + 'static> Default for LeakedRwSignal<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}
impl<T: 'static> From<LeakedRwSignal<T>> for Signal<T> {
    fn from(value: LeakedRwSignal<T>) -> Self {
        (*value).into()
    }
}

/// A version of [RwSignal] that handles cleanup using an [Rc].
///
/// This is useful when using nested signals, since keeping track of scopes can be bothersome then.
#[derive(PartialEq, Eq, Default)]
pub struct RcSignal<T: 'static> {
    inner: Rc<RcSignalInner<T>>,
}
#[derive(PartialEq, Eq, Default, Debug)]
struct RcSignalInner<T: 'static>(LeakedRwSignal<T>);
impl<T: 'static> Drop for RcSignalInner<T> {
    fn drop(&mut self) {
        self.0 .0.dispose();
    }
}
impl<T: 'static> RcSignal<T> {
    /// Creates a new [RcSignal], see type docs for more.
    #[inline(always)]
    #[track_caller]
    pub fn new(value: T) -> Self {
        Self {
            inner: Rc::new(RcSignalInner(LeakedRwSignal::new(value))),
        }
    }
    /// The returned [RwSignal] will live at least as long as a signal created here.
    ///
    /// (That is, it'll live as long as a signal owned by the current [Owner](crate::Owner).)
    pub fn into_rw(&self) -> RwSignal<T> {
        // We store the Rc, ensuring that the signal is kept at least as long as a signal that would be created here.
        let _ = store_value(self.clone());
        self.inner.0 .0
    }
}
impl<T: 'static + std::fmt::Debug> std::fmt::Debug for RcSignal<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = f.debug_struct("RcSignal");
        self.with_untracked(|v| s.field("inner", v));
        #[cfg(any(debug_assertions, feature = "ssr"))]
        s.field("defined_at", &self.inner.0.defined_at);
        s.finish()
    }
}
impl<T: 'static> Clone for RcSignal<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}
impl<T: Hash + 'static> Hash for RcSignal<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.inner.0.hash(state);
    }
}
impl<T: 'static> SignalWithUntracked for RcSignal<T> {
    type Value = T;

    fn with_untracked<O>(&self, f: impl FnOnce(&Self::Value) -> O) -> O {
        self.inner.0.with_untracked(f)
    }

    fn try_with_untracked<O>(
        &self,
        f: impl FnOnce(&Self::Value) -> O,
    ) -> Option<O> {
        self.inner.0.try_with_untracked(f)
    }
}
impl<T: 'static> SignalWith for RcSignal<T> {
    type Value = T;

    fn with<O>(&self, f: impl FnOnce(&Self::Value) -> O) -> O {
        self.inner.0.with(f)
    }

    fn try_with<O>(&self, f: impl FnOnce(&Self::Value) -> O) -> Option<O> {
        self.inner.0.try_with(f)
    }
}
impl<T: Clone + 'static> SignalGetUntracked for RcSignal<T> {
    type Value = T;

    fn get_untracked(&self) -> Self::Value {
        self.inner.0.get_untracked()
    }

    fn try_get_untracked(&self) -> Option<Self::Value> {
        self.inner.0.try_get_untracked()
    }
}
impl<T: Clone + 'static> SignalGet for RcSignal<T> {
    type Value = T;

    fn get(&self) -> Self::Value {
        self.inner.0.get()
    }

    fn try_get(&self) -> Option<Self::Value> {
        self.inner.0.try_get()
    }
}
impl<T: 'static> SignalSet for RcSignal<T> {
    type Value = T;

    fn set(&self, new_value: T) {
        self.inner.0.set(new_value);
    }

    fn try_set(&self, new_value: T) -> Option<T> {
        self.inner.0.try_set(new_value)
    }
}
impl<T: Clone + 'static> SignalUpdate for RcSignal<T> {
    type Value = T;

    fn update(&self, f: impl FnOnce(&mut Self::Value)) {
        let mut v = self.get_untracked();
        f(&mut v);
        self.inner.0.set(v);
    }

    fn try_update<O>(
        &self,
        f: impl FnOnce(&mut Self::Value) -> O,
    ) -> Option<O> {
        let mut v = self.try_get_untracked()?;
        let ret = f(&mut v);
        self.inner.0.set(v);
        Some(ret)
    }
}
