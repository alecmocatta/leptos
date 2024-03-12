use crate::{
    create_rw_signal, store_value, RwSignal, Scope, Signal, SignalDispose,
    SignalGet, SignalGetUntracked, SignalSet, SignalSetUntracked, SignalUpdate,
    SignalUpdateUntracked, SignalWith, SignalWithUntracked,
};
use std::{
    cell::Cell,
    hash::{Hash, Hasher},
    ops::Deref,
    rc::Rc,
};

thread_local! {
    static INTENTIONAL_LEAK: Cell<bool> = Cell::new(false);
}

pub(super) fn leaking_intentionally() -> bool {
    INTENTIONAL_LEAK.get()
}
fn with_intentional_leak<Out>(f: impl FnOnce() -> Out) -> Out {
    let old = INTENTIONAL_LEAK.replace(true);
    let ret = f();
    let is_true = INTENTIONAL_LEAK.replace(old);
    debug_assert!(is_true);
    ret
}

/// A leaked [RwSignal].
/// It is important that there are no ways to construct this without leaking it, including during deserialisation.
///
/// This is equivalent to a [RwSignal], but it is never disposed of unless done manually by the user
/// or the runtime itself is disposed of.
///
/// Useful to statically guarantee a global signal is not disposed of accidentally.
#[derive(PartialEq, Eq, Debug)]
pub struct LeakedRwSignal<T: 'static>(RwSignal<T>);
impl<T: 'static> LeakedRwSignal<T> {
    /// Creates a new [LeakedRwSignal], see type docs for more.
    #[inline(always)]
    #[track_caller]
    pub fn new(value: T) -> Self {
        with_intentional_leak(|| {
            Self(Scope::LEAK.with_owner(move || create_rw_signal(value)))
        })
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
#[derive(PartialEq, Eq, Default, Debug)]
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
impl<T: 'static> SignalSetUntracked<T> for RcSignal<T> {
    fn set_untracked(&self, new_value: T) {
        self.inner.0.set_untracked(new_value);
    }

    fn try_set_untracked(&self, new_value: T) -> Option<T> {
        self.inner.0.try_set_untracked(new_value)
    }
}
impl<T: Clone + 'static> SignalUpdate for RcSignal<T> {
    type Value = T;

    fn update(&self, f: impl FnOnce(&mut Self::Value)) {
        self.inner.0.update(f);
    }

    fn try_update<O>(
        &self,
        f: impl FnOnce(&mut Self::Value) -> O,
    ) -> Option<O> {
        self.inner.0.try_update(f)
    }
}
impl<T: Clone + 'static> SignalUpdateUntracked<T> for RcSignal<T> {
    fn update_untracked(&self, f: impl FnOnce(&mut T)) {
        self.inner.0.update_untracked(f);
    }

    fn try_update_untracked<O>(
        &self,
        f: impl FnOnce(&mut T) -> O,
    ) -> Option<O> {
        self.inner.0.try_update_untracked(f)
    }
}
