use crate::{
    create_isomorphic_effect, diagnostics::AccessDiagnostics, node::NodeId,
    on_cleanup, with_runtime, AnyComputation, Runtime, SignalDispose,
    SignalGet, SignalGetUntracked, SignalStream, SignalWith,
    SignalWithUntracked,
};
use std::{any::Any, cell::RefCell, fmt, marker::PhantomData, rc::Rc};

// IMPLEMENTATION NOTE:
// MemoNodedups are implemented "lazily," i.e., the inner computation is not run
// when the memo_nodedup is created or when its value is marked as stale, but on demand
// when it is accessed, if the value is stale. This means that the value is stored
// internally as Option<T>, even though it can always be accessed by the user as T.
// This means the inner value can be unwrapped in circumstances in which we know
// `Runtime::update_if_necessary()` has already been called, e.g., in the
// `.try_with_no_subscription()` calls below that are unwrapped with
// `.expect("invariant: must have already been initialized")`.

/// Creates a derived reactive value based on other reactive values.
///
/// Unlike a "derived signal," a memo_nodedup comes with two guarantees:
/// 1. The memo_nodedup will only run *once* per change, no matter how many times you
/// access its value.
/// 2. The memo_nodedup will always notify its dependents.
///
/// As with [`create_effect`](crate::create_effect), the argument to the memo_nodedup function is the previous value,
/// i.e., the current value of the memo_nodedup, which will be `None` for the initial calculation.
#[cfg_attr(
    any(debug_assertions, feature="ssr"),
    instrument(
        level = "trace",
        skip_all,
        fields(
            ty = %std::any::type_name::<T>()
        )
    )
)]
#[track_caller]
#[inline(always)]
pub fn create_memo_nodedup<T>(
    f: impl Fn(Option<&T>) -> T + 'static,
) -> MemoNodedup<T>
where
    T: 'static,
{
    Runtime::current().create_memo_nodedup(f)
}

/// A derived reactive value based on other reactive values.
///
/// Unlike a "derived signal," a memo_nodedup comes with two guarantees:
/// 1. The memo_nodedup will only run *once* per change, no matter how many times you
/// access its value.
/// 2. The memo_nodedup will always notify its dependents.
///
/// As with [`create_effect`](crate::create_effect), the argument to the memo_nodedup function is the previous value,
/// i.e., the current value of the memo_nodedup, which will be `None` for the initial calculation.
///
/// ## Core Trait Implementations
/// - [`.get()`](#impl-SignalGet<T>-for-MemoNodedup<T>) (or calling the signal as a function) clones the current
///   value of the signal. If you call it within an effect, it will cause that effect
///   to subscribe to the signal, and to re-run whenever the value of the signal changes.
///   - [`.get_untracked()`](#impl-SignalGetUntracked<T>-for-MemoNodedup<T>) clones the value of the signal
///   without reactively tracking it.
/// - [`.with()`](#impl-SignalWith<T>-for-MemoNodedup<T>) allows you to reactively access the signal’s value without
///   cloning by applying a callback function.
///   - [`.with_untracked()`](#impl-SignalWithUntracked<T>-for-MemoNodedup<T>) allows you to access the signal’s
///   value without reactively tracking it.
/// - [`.to_stream()`](#impl-SignalStream<T>-for-MemoNodedup<T>) converts the signal to an `async` stream of values.
pub struct MemoNodedup<T>
where
    T: 'static,
{
    pub(crate) id: NodeId,
    pub(crate) ty: PhantomData<T>,
    #[cfg(any(debug_assertions, feature = "ssr"))]
    pub(crate) defined_at: &'static std::panic::Location<'static>,
}

impl<T> MemoNodedup<T> {
    /// Creates a new memo_nodedup from the given function.
    ///
    /// This is identical to [`create_memo_nodedup`].
    #[inline(always)]
    #[track_caller]
    pub fn new(f: impl Fn(Option<&T>) -> T + 'static) -> MemoNodedup<T>
    where
        T: 'static,
    {
        create_memo_nodedup(f)
    }
}

impl<T> Clone for MemoNodedup<T>
where
    T: 'static,
{
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for MemoNodedup<T> {}

impl<T> fmt::Debug for MemoNodedup<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut s = f.debug_struct("MemoNodedup");
        s.field("id", &self.id);
        s.field("ty", &self.ty);
        #[cfg(any(debug_assertions, feature = "ssr"))]
        s.field("defined_at", &self.defined_at);
        s.finish()
    }
}

impl<T> Eq for MemoNodedup<T> {}

impl<T> PartialEq for MemoNodedup<T> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

fn forward_ref_to<T, O, F: FnOnce(&T) -> O>(
    f: F,
) -> impl FnOnce(&Option<T>) -> O {
    |maybe_value: &Option<T>| {
        let ref_t = maybe_value
            .as_ref()
            .expect("invariant: must have already been initialized");
        f(ref_t)
    }
}

impl<T: Clone> SignalGetUntracked for MemoNodedup<T> {
    type Value = T;

    #[cfg_attr(
        any(debug_assertions, feature = "ssr"),
        instrument(
            level = "trace",
            name = "MemoNodedup::get_untracked()",
            skip_all,
            fields(
                id = ?self.id,
                defined_at = %self.defined_at,
                ty = %std::any::type_name::<T>()
            )
        )
    )]
    fn get_untracked(&self) -> T {
        with_runtime(move |runtime| {
            let f = |maybe_value: &Option<T>| {
                maybe_value
                    .clone()
                    .expect("invariant: must have already been initialized")
            };
            match self.id.try_with_no_subscription(runtime, f) {
                Ok(t) => t,
                Err(_) => panic_getting_dead_memo_nodedup(
                    #[cfg(any(debug_assertions, feature = "ssr"))]
                    self.defined_at,
                ),
            }
        })
        .expect("runtime to be alive")
    }

    #[cfg_attr(
        any(debug_assertions, feature = "ssr"),
        instrument(
            level = "trace",
            name = "MemoNodedup::try_get_untracked()",
            skip_all,
            fields(
                id = ?self.id,
                defined_at = %self.defined_at,
                ty = %std::any::type_name::<T>()
            )
        )
    )]
    #[inline(always)]
    fn try_get_untracked(&self) -> Option<T> {
        self.try_with_untracked(T::clone)
    }
}

impl<T> SignalWithUntracked for MemoNodedup<T> {
    type Value = T;

    #[cfg_attr(
        any(debug_assertions, feature = "ssr"),
        instrument(
            level = "trace",
            name = "MemoNodedup::with_untracked()",
            skip_all,
            fields(
                id = ?self.id,
                defined_at = %self.defined_at,
                ty = %std::any::type_name::<T>()
            )
        )
    )]
    fn with_untracked<O>(&self, f: impl FnOnce(&T) -> O) -> O {
        with_runtime(|runtime| {
            match self.id.try_with_no_subscription(runtime, forward_ref_to(f)) {
                Ok(t) => t,
                Err(_) => panic_getting_dead_memo_nodedup(
                    #[cfg(any(debug_assertions, feature = "ssr"))]
                    self.defined_at,
                ),
            }
        })
        .expect("runtime to be alive")
    }

    #[cfg_attr(
        any(debug_assertions, feature = "ssr"),
        instrument(
            level = "trace",
            name = "MemoNodedup::try_with_untracked()",
            skip_all,
            fields(
                id = ?self.id,
                defined_at = %self.defined_at,
                ty = %std::any::type_name::<T>()
            )
        )
    )]
    #[inline]
    fn try_with_untracked<O>(&self, f: impl FnOnce(&T) -> O) -> Option<O> {
        with_runtime(|runtime| {
            self.id.try_with_no_subscription(runtime, |v: &T| f(v)).ok()
        })
        .ok()
        .flatten()
    }
}

impl<T: Clone> SignalGet for MemoNodedup<T> {
    type Value = T;

    #[cfg_attr(
        any(debug_assertions, feature = "ssr"),
        instrument(
            name = "MemoNodedup::get()",
            level = "trace",
            skip_all,
            fields(
                id = ?self.id,
                defined_at = %self.defined_at,
                ty = %std::any::type_name::<T>()
            )
        )
    )]
    #[track_caller]
    #[inline(always)]
    fn get(&self) -> T {
        self.with(T::clone)
    }

    #[cfg_attr(
        any(debug_assertions, feature = "ssr"),
        instrument(
            level = "trace",
            name = "MemoNodedup::try_get()",
            skip_all,
            fields(
                id = ?self.id,
                defined_at = %self.defined_at,
                ty = %std::any::type_name::<T>()
            )
        )
    )]
    #[track_caller]
    #[inline(always)]
    fn try_get(&self) -> Option<T> {
        self.try_with(T::clone)
    }
}

impl<T> SignalWith for MemoNodedup<T> {
    type Value = T;

    #[cfg_attr(
        any(debug_assertions, feature = "ssr"),
        instrument(
            level = "trace",
            name = "MemoNodedup::with()",
            skip_all,
            fields(
                id = ?self.id,
                defined_at = %self.defined_at,
                ty = %std::any::type_name::<T>()
            )
        )
    )]
    #[track_caller]
    fn with<O>(&self, f: impl FnOnce(&T) -> O) -> O {
        match self.try_with(f) {
            Some(t) => t,
            None => panic_getting_dead_memo_nodedup(
                #[cfg(any(debug_assertions, feature = "ssr"))]
                self.defined_at,
            ),
        }
    }

    #[cfg_attr(
        any(debug_assertions, feature = "ssr"),
        instrument(
            level = "trace",
            name = "MemoNodedup::try_with()",
            skip_all,
            fields(
                id = ?self.id,
                defined_at = %self.defined_at,
                ty = %std::any::type_name::<T>()
            )
        )
    )]
    #[track_caller]
    fn try_with<O>(&self, f: impl FnOnce(&T) -> O) -> Option<O> {
        let diagnostics = diagnostics!(self);

        with_runtime(|runtime| {
            self.id.subscribe(runtime, diagnostics);
            self.id
                .try_with_no_subscription(runtime, forward_ref_to(f))
                .ok()
        })
        .ok()
        .flatten()
    }
}

impl<T: Clone> SignalStream<T> for MemoNodedup<T> {
    #[cfg_attr(
        any(debug_assertions, feature = "ssr"),
        instrument(
            level = "trace",
            name = "MemoNodedup::to_stream()",
            skip_all,
            fields(
                id = ?self.id,
                defined_at = %self.defined_at,
                ty = %std::any::type_name::<T>()
            )
        )
    )]
    fn to_stream(&self) -> std::pin::Pin<Box<dyn futures::Stream<Item = T>>> {
        let (tx, rx) = futures::channel::mpsc::unbounded();

        let close_channel = tx.clone();

        on_cleanup(move || close_channel.close_channel());

        let this = *self;

        create_isomorphic_effect(move |_| {
            let _ = tx.unbounded_send(this.get());
        });

        Box::pin(rx)
    }
}

impl<T> SignalDispose for MemoNodedup<T> {
    fn dispose(self) {
        _ = with_runtime(|runtime| runtime.dispose_node(self.id));
    }
}

impl_get_fn_traits![MemoNodedup];

pub(crate) struct MemoNodedupState<T, F>
where
    T: 'static,
    F: Fn(Option<&T>) -> T,
{
    pub f: F,
    pub t: PhantomData<T>,
    #[cfg(any(debug_assertions, feature = "ssr"))]
    pub(crate) defined_at: &'static std::panic::Location<'static>,
}

impl<T, F> AnyComputation for MemoNodedupState<T, F>
where
    T: 'static,
    F: Fn(Option<&T>) -> T,
{
    #[cfg_attr(
        any(debug_assertions, feature = "ssr"),
        instrument(
            name = "MemoNodedup::run()",
            level = "debug",
            skip_all,
            fields(
              defined_at = %self.defined_at,
              ty = %std::any::type_name::<T>()
            )
        )
    )]
    fn run(&self, value: Rc<RefCell<dyn Any>>) -> bool {
        let new_value = {
            let value = value.borrow();
            let curr_value = value
                .downcast_ref::<Option<T>>()
                .expect("to downcast memo_nodedup value");

            // run the effect
            let new_value = (self.f)(curr_value.as_ref());
            new_value
        };

        {
            let mut value = value.borrow_mut();
            let curr_value = value
                .downcast_mut::<Option<T>>()
                .expect("to downcast memo_nodedup value");
            *curr_value = Some(new_value);
        }

        true
    }
}

#[cold]
#[inline(never)]
#[track_caller]
fn format_memo_nodedup_warning(
    msg: &str,
    #[cfg(any(debug_assertions, feature = "ssr"))]
    defined_at: &'static std::panic::Location<'static>,
) -> String {
    let location = std::panic::Location::caller();

    let defined_at_msg = {
        #[cfg(any(debug_assertions, feature = "ssr"))]
        {
            format!("signal created here: {defined_at}\n")
        }

        #[cfg(not(any(debug_assertions, feature = "ssr")))]
        {
            String::default()
        }
    };

    format!("{msg}\n{defined_at_msg}warning happened here: {location}",)
}

#[cold]
#[inline(never)]
#[track_caller]
pub(crate) fn panic_getting_dead_memo_nodedup(
    #[cfg(any(debug_assertions, feature = "ssr"))]
    defined_at: &'static std::panic::Location<'static>,
) -> ! {
    panic!(
        "{}",
        format_memo_nodedup_warning(
            "Attempted to get a memo_nodedup after it was disposed.",
            #[cfg(any(debug_assertions, feature = "ssr"))]
            defined_at,
        )
    )
}
